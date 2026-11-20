//! Package documentation topology, evidence, and projections (#14122).
//!
//! Composes `cargo metadata`, `[workspace.metadata.publish.allow]`, and
//! `policy/package-documentation.toml`. External docs.rs / public-API proof is
//! referenced as `NOT_PROVEN` rather than reimplemented.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail, eyre};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const POLICY_PATH: &str = "policy/package-documentation.toml";
pub const JSON_PATH: &str = "docs/reference/package-documentation.json";
pub const MD_PATH: &str = "docs/reference/package-documentation.md";

const EXPECTED_CONTRACT: &str = "package_documentation.v1";
const WORKSPACE_REPO: &str = env!("CARGO_PKG_REPOSITORY");

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    Pass,
    Fail,
    NotApplicable,
    NotProven,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvidenceCell {
    pub status: EvidenceStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authority: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackageRow {
    pub name: String,
    pub manifest_path: String,
    pub class: String,
    pub repair_cohort: u8,
    pub in_publish_allowlist: bool,
    pub requirements: BTreeMap<String, EvidenceCell>,
    pub observations: BTreeMap<String, EvidenceCell>,
}

#[derive(Debug, Serialize)]
pub struct MatrixDocument {
    pub schema_version: String,
    pub contract: String,
    pub denominator_authority: String,
    pub publish_allowlist_authority: String,
    pub package_count: usize,
    pub publish_allowlist_count: usize,
    pub root_readme_crate_table: String,
    pub packages: Vec<PackageRow>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyDocument {
    schema_version: String,
    contract: String,
    root_readme: RootReadmePolicy,
    package_class: Vec<PackageClassPolicy>,
    #[serde(default)]
    package_disposition: Vec<PackageDisposition>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RootReadmePolicy {
    crate_table: String,
    owner: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PackageClassPolicy {
    id: String,
    description: String,
    repair_cohort: u8,
    #[serde(default)]
    required: Vec<String>,
    #[serde(default)]
    optional: Vec<String>,
    #[serde(default)]
    observations: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PackageDisposition {
    name: String,
    class: String,
    #[serde(default)]
    notes: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    workspace_members: Vec<String>,
    workspace_root: String,
}

#[derive(Debug, serde::Deserialize)]
struct CargoPackage {
    id: String,
    name: String,
    manifest_path: String,
    description: Option<String>,
    documentation: Option<String>,
    repository: Option<String>,
    readme: Option<String>,
    #[allow(dead_code)]
    publish: Option<Vec<String>>,
    targets: Vec<CargoTarget>,
}

#[derive(Debug, serde::Deserialize)]
struct CargoTarget {
    #[allow(dead_code)]
    name: String,
    kind: Vec<String>,
}

pub fn run(check: bool, strict: bool) -> Result<()> {
    let root = project_root()?;
    let policy = load_policy(&root)?;
    let metadata = load_metadata(&root)?;
    let allow = load_publish_allowlist(&metadata)?;
    let matrix = build_matrix(&root, &policy, &metadata, &allow)?;
    validate_policy_reconciliation(&policy, &metadata, &allow)?;
    if strict {
        validate_rows(&policy, &matrix, &allow)?;
    }

    let json = serde_json::to_string_pretty(&matrix)
        .context("serializing package documentation matrix")?;
    let markdown = render_markdown(&matrix);

    let json_path = root.join(JSON_PATH);
    let md_path = root.join(MD_PATH);

    if check {
        let existing_json = fs::read_to_string(&json_path)
            .wrap_err_with(|| format!("failed to read {JSON_PATH}"))?;
        let existing_md =
            fs::read_to_string(&md_path).wrap_err_with(|| format!("failed to read {MD_PATH}"))?;
        if normalize_newlines(&existing_json) != normalize_newlines(&json) {
            bail!("{JSON_PATH} is stale; run `cargo xtask package-documentation`");
        }
        if normalize_newlines(&existing_md) != normalize_newlines(&markdown) {
            bail!("{MD_PATH} is stale; run `cargo xtask package-documentation`");
        }
        println!(
            "package documentation matrix is valid{}: {} packages, {} allowlisted",
            if strict { " (strict evidence)" } else { "" },
            matrix.package_count,
            matrix.publish_allowlist_count
        );
        return Ok(());
    }

    if let Some(parent) = json_path.parent() {
        fs::create_dir_all(parent)
            .wrap_err_with(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&json_path, json).wrap_err_with(|| format!("failed to write {JSON_PATH}"))?;
    fs::write(&md_path, markdown).wrap_err_with(|| format!("failed to write {MD_PATH}"))?;
    println!("wrote {JSON_PATH} and {MD_PATH} for {} packages", matrix.package_count);
    Ok(())
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn load_policy(root: &Path) -> Result<PolicyDocument> {
    let text = fs::read_to_string(root.join(POLICY_PATH))
        .wrap_err_with(|| format!("failed to read {POLICY_PATH}"))?;
    let policy: PolicyDocument =
        toml::from_str(&text).wrap_err_with(|| format!("failed to parse {POLICY_PATH}"))?;
    if policy.contract != EXPECTED_CONTRACT {
        bail!("{POLICY_PATH}: contract {:?} != expected {EXPECTED_CONTRACT}", policy.contract);
    }
    Ok(policy)
}

fn load_metadata(root: &Path) -> Result<CargoMetadata> {
    let output = Command::new("cargo")
        .arg("metadata")
        .arg("--no-deps")
        .arg("--format-version")
        .arg("1")
        .current_dir(root)
        .output()
        .context("running cargo metadata")?;
    if !output.status.success() {
        bail!("cargo metadata failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    serde_json::from_slice(&output.stdout).context("parsing cargo metadata json")
}

fn load_publish_allowlist(metadata: &CargoMetadata) -> Result<Vec<String>> {
    let root_manifest = PathBuf::from(&metadata.workspace_root).join("Cargo.toml");
    let root_toml: toml::Value =
        toml::from_str(&fs::read_to_string(&root_manifest).context("reading root Cargo.toml")?)
            .context("parsing root Cargo.toml")?;
    let allow = root_toml
        .get("workspace")
        .and_then(|w| w.get("metadata"))
        .and_then(|m| m.get("publish"))
        .and_then(|p| p.get("allow"))
        .and_then(|v| v.as_array())
        .ok_or_else(|| eyre!("missing [workspace.metadata.publish.allow]"))?
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    if allow.is_empty() {
        bail!("publish allowlist is empty");
    }
    Ok(allow)
}

fn workspace_members(metadata: &CargoMetadata) -> Result<Vec<&CargoPackage>> {
    let member_ids: BTreeSet<&str> =
        metadata.workspace_members.iter().map(String::as_str).collect();
    let mut members: Vec<&CargoPackage> =
        metadata.packages.iter().filter(|p| member_ids.contains(p.id.as_str())).collect();
    members.sort_by_key(|p| p.name.as_str());
    Ok(members)
}

fn class_map(policy: &PolicyDocument) -> BTreeMap<&str, &PackageClassPolicy> {
    policy.package_class.iter().map(|c| (c.id.as_str(), c)).collect()
}

fn disposition_map(policy: &PolicyDocument) -> BTreeMap<&str, &PackageDisposition> {
    policy.package_disposition.iter().map(|d| (d.name.as_str(), d)).collect()
}

fn derive_class(
    package: &CargoPackage,
    allow: &BTreeSet<&str>,
    dispositions: &BTreeMap<&str, &PackageDisposition>,
) -> Result<String> {
    if let Some(row) = dispositions.get(package.name.as_str()) {
        return Ok(row.class.clone());
    }
    if allow.contains(package.name.as_str()) {
        let has_lib = package.targets.iter().any(|t| t.kind.iter().any(|k| k == "lib"));
        let has_bin = package.targets.iter().any(|t| t.kind.iter().any(|k| k == "bin"));
        if has_bin && !has_lib {
            return Ok("published_binary".to_owned());
        }
        return Ok("published_library".to_owned());
    }
    Ok("internal_product_crate".to_owned())
}

fn package_root(manifest_path: &str) -> PathBuf {
    PathBuf::from(manifest_path).parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

fn readme_resolves(package: &CargoPackage) -> (bool, Option<String>) {
    let root = package_root(&package.manifest_path);
    let readme_name = package.readme.as_deref().unwrap_or("README.md");
    let path = root.join(readme_name);
    (path.is_file(), Some(path.display().to_string()))
}

fn has_package_readme(package: &CargoPackage) -> bool {
    readme_resolves(package).0
}

fn crate_root_has_docs(package: &CargoPackage) -> bool {
    let root = package_root(&package.manifest_path);
    for rel in ["src/lib.rs", "src/main.rs", "src/bin"] {
        let path = root.join(rel);
        if path.is_file() {
            if file_has_rustdoc(&path) {
                return true;
            }
        } else if rel == "src/bin" && path.is_dir() {
            if let Ok(entries) = fs::read_dir(path) {
                for entry in entries.flatten() {
                    if entry.path().extension().is_some_and(|e| e == "rs")
                        && file_has_rustdoc(&entry.path())
                    {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn file_has_rustdoc(path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    for line in text.lines().take(80) {
        let trimmed = line.trim();
        if trimmed.starts_with("//!") || trimmed.starts_with("/*!") {
            return true;
        }
        if trimmed.starts_with("///") || trimmed.starts_with("/**") {
            return true;
        }
        if !trimmed.is_empty() && !trimmed.starts_with('#') && !trimmed.starts_with("//") {
            break;
        }
    }
    false
}

fn has_examples_surface(package: &CargoPackage) -> bool {
    let root = package_root(&package.manifest_path);
    let examples = root.join("examples");
    examples.is_dir() && fs::read_dir(examples).map(|mut d| d.next().is_some()).unwrap_or(false)
}

fn manifest_has_docs_rs_section(manifest_path: &str) -> bool {
    let Ok(text) = fs::read_to_string(manifest_path) else {
        return false;
    };
    text.contains("[package.metadata.docs.rs]")
}

fn contributor_guidance_present(package: &CargoPackage) -> bool {
    let root = package_root(&package.manifest_path);
    root.join("AGENTS.md").is_file() || root.join("CLAUDE.md").is_file()
}

fn repository_matches_workspace(repo: Option<&str>) -> bool {
    match repo {
        None => false,
        Some(value) => value == WORKSPACE_REPO,
    }
}

fn not_proven_observation(authority: &str) -> EvidenceCell {
    EvidenceCell {
        status: EvidenceStatus::NotProven,
        detail: None,
        authority: Some(authority.to_owned()),
    }
}

fn evaluate_requirement(
    key: &str,
    package: &CargoPackage,
    class_id: &str,
    disposition: Option<&PackageDisposition>,
) -> EvidenceCell {
    match key {
        "package_readme" => EvidenceCell {
            status: if has_package_readme(package) {
                EvidenceStatus::Pass
            } else {
                EvidenceStatus::Fail
            },
            detail: package.readme.clone(),
            authority: None,
        },
        "crate_root_docs" => EvidenceCell {
            status: if crate_root_has_docs(package) {
                EvidenceStatus::Pass
            } else {
                EvidenceStatus::Fail
            },
            detail: None,
            authority: None,
        },
        "manifest_description" => EvidenceCell {
            status: if package.description.as_ref().is_some_and(|d| !d.trim().is_empty()) {
                EvidenceStatus::Pass
            } else {
                EvidenceStatus::Fail
            },
            detail: None,
            authority: None,
        },
        "manifest_documentation" => EvidenceCell {
            status: if package.documentation.as_ref().is_some_and(|d| !d.trim().is_empty()) {
                EvidenceStatus::Pass
            } else {
                EvidenceStatus::Fail
            },
            detail: package.documentation.clone(),
            authority: None,
        },
        "manifest_repository" => EvidenceCell {
            status: if repository_matches_workspace(package.repository.as_deref()) {
                EvidenceStatus::Pass
            } else {
                EvidenceStatus::Fail
            },
            detail: package.repository.clone(),
            authority: Some(WORKSPACE_REPO.to_owned()),
        },
        "readme_path_resolves" => {
            let (ok, path) = readme_resolves(package);
            EvidenceCell {
                status: if ok { EvidenceStatus::Pass } else { EvidenceStatus::Fail },
                detail: path,
                authority: None,
            }
        }
        "examples_surface" => EvidenceCell {
            status: if has_examples_surface(package) {
                EvidenceStatus::Pass
            } else {
                EvidenceStatus::NotApplicable
            },
            detail: None,
            authority: None,
        },
        "docs_rs_metadata" => EvidenceCell {
            status: if manifest_has_docs_rs_section(&package.manifest_path) {
                EvidenceStatus::Pass
            } else {
                EvidenceStatus::NotApplicable
            },
            detail: None,
            authority: None,
        },
        "contributor_guidance" => EvidenceCell {
            status: if contributor_guidance_present(package) {
                EvidenceStatus::Pass
            } else {
                EvidenceStatus::NotApplicable
            },
            detail: None,
            authority: None,
        },
        "exclusion_rationale" => EvidenceCell {
            status: if class_id == "explicitly_excluded"
                && disposition.and_then(|d| d.notes.as_ref()).is_some_and(|n| !n.trim().is_empty())
            {
                EvidenceStatus::Pass
            } else if class_id == "explicitly_excluded" {
                EvidenceStatus::Fail
            } else {
                EvidenceStatus::NotApplicable
            },
            detail: disposition.and_then(|d| d.notes.clone()),
            authority: None,
        },
        other => EvidenceCell {
            status: EvidenceStatus::Fail,
            detail: Some(format!("unknown requirement key {other:?}")),
            authority: None,
        },
    }
}

fn build_matrix(
    _root: &Path,
    policy: &PolicyDocument,
    metadata: &CargoMetadata,
    allow: &[String],
) -> Result<MatrixDocument> {
    let classes = class_map(policy);
    let dispositions = disposition_map(policy);
    let allow_set: BTreeSet<&str> = allow.iter().map(String::as_str).collect();
    let members = workspace_members(metadata)?;

    let mut packages = Vec::with_capacity(members.len());
    for package in members {
        let class_id = derive_class(package, &allow_set, &dispositions)?;
        let class = classes
            .get(class_id.as_str())
            .ok_or_else(|| eyre!("unknown class {class_id:?} for {}", package.name))?;
        let disposition = dispositions.get(package.name.as_str()).copied();

        let mut requirements = BTreeMap::new();
        for key in class.required.iter().chain(class.optional.iter()).cloned() {
            requirements
                .insert(key.clone(), evaluate_requirement(&key, package, &class_id, disposition));
        }

        let mut observations = BTreeMap::new();
        for key in &class.observations {
            let cell = match key.as_str() {
                "builds_rustdoc" => not_proven_observation("cargo doc / scripts/verify-docs-rs.sh"),
                "public_api_documented" => {
                    not_proven_observation(".ci/public-api-baselines and missing-docs policy")
                }
                "docs_rs_build" => not_proven_observation("scripts/verify-docs-rs.sh"),
                "usable_from_docs" => not_proven_observation("docs.rs build + entrypoint review"),
                other => EvidenceCell {
                    status: EvidenceStatus::NotProven,
                    detail: Some(format!("unknown observation {other:?}")),
                    authority: None,
                },
            };
            observations.insert(key.clone(), cell);
        }

        // Internal crates must not advertise a docs.rs URL as if they were published product.
        if !allow_set.contains(package.name.as_str())
            && package.documentation.as_deref().is_some_and(|d| d.contains("docs.rs"))
        {
            requirements.insert(
                "internal_product_claim_boundary".to_owned(),
                EvidenceCell {
                    status: EvidenceStatus::Fail,
                    detail: package.documentation.clone(),
                    authority: Some("#14122 negative control 6".to_owned()),
                },
            );
        }

        packages.push(PackageRow {
            name: package.name.clone(),
            manifest_path: package.manifest_path.clone(),
            class: class_id,
            repair_cohort: class.repair_cohort,
            in_publish_allowlist: allow_set.contains(package.name.as_str()),
            requirements,
            observations,
        });
    }

    Ok(MatrixDocument {
        schema_version: policy.schema_version.clone(),
        contract: policy.contract.clone(),
        denominator_authority: "cargo metadata workspace_members".to_owned(),
        publish_allowlist_authority: "Cargo.toml#workspace.metadata.publish.allow".to_owned(),
        package_count: packages.len(),
        publish_allowlist_count: allow.len(),
        root_readme_crate_table: policy.root_readme.crate_table.clone(),
        packages,
    })
}

fn validate_policy_reconciliation(
    policy: &PolicyDocument,
    metadata: &CargoMetadata,
    allow: &[String],
) -> Result<()> {
    let member_names: BTreeSet<String> =
        workspace_members(metadata)?.into_iter().map(|p| p.name.clone()).collect();
    for disposition in &policy.package_disposition {
        if !member_names.contains(&disposition.name) {
            bail!(
                "policy disposition references removed or unknown package {:?}",
                disposition.name
            );
        }
        if !policy.package_class.iter().any(|c| c.id == disposition.class) {
            bail!(
                "policy disposition for {:?} names unknown class {:?}",
                disposition.name,
                disposition.class
            );
        }
    }
    for crate_name in allow {
        if !member_names.contains(crate_name) {
            bail!(
                "publish allowlist crate {crate_name:?} is absent from workspace members (documentation matrix denominator)"
            );
        }
    }
    if policy.root_readme.crate_table != "curated"
        && policy.root_readme.crate_table != "topology_derived"
    {
        bail!(
            "root_readme.crate_table must be curated or topology_derived, got {:?}",
            policy.root_readme.crate_table
        );
    }
    Ok(())
}

fn validate_rows(policy: &PolicyDocument, matrix: &MatrixDocument, allow: &[String]) -> Result<()> {
    let classes = class_map(policy);
    let allow_set: BTreeSet<&str> = allow.iter().map(String::as_str).collect();
    let mut errors = Vec::new();

    if matrix.package_count != matrix.packages.len() {
        errors.push("package_count contradicts packages vec length".to_owned());
    }

    for row in &matrix.packages {
        let Some(class) = classes.get(row.class.as_str()) else {
            errors.push(format!("{}: unknown class {:?}", row.name, row.class));
            continue;
        };
        for key in &class.required {
            match row.requirements.get(key) {
                Some(cell) if cell.status == EvidenceStatus::Pass => {}
                Some(cell) => {
                    errors.push(format!("{}: required {key} is {:?}", row.name, cell.status))
                }
                None => errors.push(format!("{}: missing required key {key}", row.name)),
            }
        }
        if allow_set.contains(row.name.as_str()) != row.in_publish_allowlist {
            errors.push(format!("{}: publish allowlist flag contradicts topology", row.name));
        }
        if row.in_publish_allowlist
            && row.class != "published_library"
            && row.class != "published_binary"
        {
            errors.push(format!(
                "{}: allowlisted crate must classify as published_* not {}",
                row.name, row.class
            ));
        }
    }

    if !errors.is_empty() {
        bail!("package documentation validation failed:\n- {}", errors.join("\n- "));
    }
    Ok(())
}

fn render_markdown(matrix: &MatrixDocument) -> String {
    let mut out = String::new();
    out.push_str("<!-- auto-generated by `cargo xtask package-documentation`; do not edit -->\n\n");
    out.push_str("# Package documentation matrix\n\n");
    let _ = writeln!(
        out,
        "Denominator: {} workspace packages from `{}`. Publish allowlist: {} crates from `{}`. Root README crate table: **{}**.\n",
        matrix.package_count,
        matrix.denominator_authority,
        matrix.publish_allowlist_count,
        matrix.publish_allowlist_authority,
        matrix.root_readme_crate_table
    );
    out.push_str(
        "| Package | Class | Cohort | Allowlisted | README | Crate docs | Description |\n",
    );
    out.push_str("|---|---|---:|---|---|---|---|\n");
    for row in &matrix.packages {
        let readme = row
            .requirements
            .get("package_readme")
            .map(|c| format!("{:?}", c.status))
            .unwrap_or_else(|| "n/a".to_owned());
        let docs = row
            .requirements
            .get("crate_root_docs")
            .map(|c| format!("{:?}", c.status))
            .unwrap_or_else(|| "n/a".to_owned());
        let desc = row
            .requirements
            .get("manifest_description")
            .map(|c| format!("{:?}", c.status))
            .unwrap_or_else(|| "n/a".to_owned());
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | {} | {} | {} |",
            row.name, row.class, row.repair_cohort, row.in_publish_allowlist, readme, docs, desc
        );
    }
    out.push_str("\nObservations such as `builds_rustdoc`, `public_api_documented`, and `docs_rs_build` remain `NOT_PROVEN` in this matrix unless consumed from their owning proof planes.\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_policy(with_stale: bool) -> PolicyDocument {
        let mut policy: PolicyDocument = toml::from_str(
            r#"
schema_version = "package_documentation.v1"
contract = "package_documentation.v1"

[root_readme]
crate_table = "curated"
owner = "issue-14122"

[[package_class]]
id = "published_library"
description = "published"
repair_cohort = 1
required = ["manifest_description"]
optional = []
observations = []

[[package_class]]
id = "internal_tooling_test"
description = "tooling"
repair_cohort = 5
required = []
optional = []
observations = []
"#,
        )
        .expect("sample policy");
        if with_stale {
            policy.package_disposition.push(PackageDisposition {
                name: "removed-crate".to_owned(),
                class: "internal_tooling_test".to_owned(),
                notes: None,
            });
        }
        policy
    }

    fn sample_metadata() -> CargoMetadata {
        CargoMetadata {
            packages: vec![CargoPackage {
                id: "demo".to_owned(),
                name: "demo".to_owned(),
                manifest_path: "/tmp/demo/Cargo.toml".to_owned(),
                description: Some("demo".to_owned()),
                documentation: Some("https://docs.rs/demo".to_owned()),
                repository: Some(WORKSPACE_REPO.to_owned()),
                readme: Some("README.md".to_owned()),
                publish: None,
                targets: vec![CargoTarget {
                    name: "demo".to_owned(),
                    kind: vec!["lib".to_owned()],
                }],
            }],
            workspace_members: vec!["demo".to_owned()],
            workspace_root: "/tmp".to_owned(),
        }
    }

    #[test]
    fn rejects_stale_disposition_rows() {
        let policy = sample_policy(true);
        let metadata = sample_metadata();
        let allow = vec!["demo".to_owned()];
        let err = validate_policy_reconciliation(&policy, &metadata, &allow).unwrap_err();
        assert!(err.to_string().contains("removed or unknown package"));
    }

    #[test]
    fn rejects_allowlist_entries_absent_from_workspace() {
        let policy = sample_policy(false);
        let metadata = sample_metadata();
        let allow = vec!["demo".to_owned(), "ghost".to_owned()];
        let err = validate_policy_reconciliation(&policy, &metadata, &allow).unwrap_err();
        assert!(err.to_string().contains("absent from workspace members"), "{err}");
    }
}
