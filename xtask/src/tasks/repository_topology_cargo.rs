//! Deterministic workspace package and direct-dependency views from Cargo metadata (#8412).
//!
//! Extends `cargo xtask repo-topology` without duplicating publication authority:
//! publish disposition and repository ownership come from `policy/repository-topology.toml`
//! after the existing manifest reconciliation pass.

use super::repository_topology::{
    ManifestPackage, PublishDisposition, Topology, normalize_newlines,
};
use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail, eyre};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::process::Command;

pub const CARGO_TOPOLOGY_JSON: &str = "docs/architecture/workspace-cargo-topology.v1.json";
pub const CARGO_TOPOLOGY_MD: &str = "docs/architecture/workspace-cargo-topology.md";
const SCHEMA: &str = "workspace_cargo_topology.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceCargoTopology {
    pub schema_version: String,
    pub generated_by: String,
    pub package_count: usize,
    pub workspace_internal_edge_count: usize,
    pub packages: Vec<TopologyPackageRow>,
    pub direct_dependency_edges: Vec<DirectDependencyEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopologyPackageRow {
    pub name: String,
    pub version: String,
    pub path: String,
    pub publish_disposition: String,
    pub repository_owner: String,
    pub integration_mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectDependencyEdge {
    pub from: String,
    pub to: String,
    pub dependency_kind: String,
    pub optional: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub to_scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FullCargoMetadata {
    packages: Vec<FullCargoPackage>,
    workspace_members: Vec<String>,
    resolve: CargoResolve,
}

#[derive(Debug, Deserialize)]
struct CargoResolve {
    nodes: Vec<CargoResolveNode>,
}

#[derive(Debug, Deserialize)]
struct CargoResolveNode {
    id: String,
    deps: Vec<CargoResolvedDependency>,
}

#[derive(Debug, Deserialize)]
struct CargoResolvedDependency {
    name: String,
    pkg: String,
    dep_kinds: Vec<CargoResolvedDependencyKind>,
}

#[derive(Debug, Deserialize)]
struct CargoResolvedDependencyKind {
    kind: Option<String>,
    #[serde(default)]
    target: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FullCargoPackage {
    id: String,
    name: String,
    version: String,
    manifest_path: String,
    #[serde(default)]
    dependencies: Vec<DeclaredDependency>,
}

#[derive(Debug, Clone, Deserialize)]
struct DeclaredDependency {
    name: String,
    #[serde(default)]
    optional: bool,
    #[serde(default)]
    rename: Option<String>,
}

pub fn write_or_check_cargo_topology(
    check: bool,
    topology: &Topology,
    manifests: &[ManifestPackage],
) -> Result<()> {
    let root = project_root()?;
    let view = build_workspace_cargo_topology(&root, topology, manifests)?;
    let json = render_json(&view)?;
    let markdown = render_markdown(&view);

    let json_path = root.join(CARGO_TOPOLOGY_JSON);
    let md_path = root.join(CARGO_TOPOLOGY_MD);

    if check {
        let committed_json = fs::read_to_string(&json_path)
            .wrap_err_with(|| format!("failed to read {CARGO_TOPOLOGY_JSON}"))?;
        if normalize_newlines(&committed_json) != json {
            bail!("{CARGO_TOPOLOGY_JSON} is stale; run `cargo xtask repo-topology`");
        }
        let committed_md = fs::read_to_string(&md_path)
            .wrap_err_with(|| format!("failed to read {CARGO_TOPOLOGY_MD}"))?;
        if normalize_newlines(&committed_md) != markdown {
            bail!("{CARGO_TOPOLOGY_MD} is stale; run `cargo xtask repo-topology`");
        }
        return Ok(());
    }

    for path in [json_path.parent(), md_path.parent()].into_iter().flatten() {
        fs::create_dir_all(path)
            .wrap_err_with(|| format!("failed to create {}", path.display()))?;
    }
    fs::write(&json_path, &json)
        .wrap_err_with(|| format!("failed to write {CARGO_TOPOLOGY_JSON}"))?;
    fs::write(&md_path, &markdown)
        .wrap_err_with(|| format!("failed to write {CARGO_TOPOLOGY_MD}"))?;
    Ok(())
}

fn build_workspace_cargo_topology(
    root: &Path,
    topology: &Topology,
    manifests: &[ManifestPackage],
) -> Result<WorkspaceCargoTopology> {
    let metadata = load_full_cargo_metadata(root)?;
    let policy_rows: BTreeMap<&str, &super::repository_topology::Package> =
        topology.packages.iter().map(|package| (package.name.as_str(), package)).collect();

    let member_ids: BTreeSet<&str> =
        metadata.workspace_members.iter().map(String::as_str).collect();
    let packages_by_id: BTreeMap<&str, &FullCargoPackage> = metadata
        .packages
        .iter()
        .filter(|package| member_ids.contains(package.id.as_str()))
        .map(|package| (package.id.as_str(), package))
        .collect();

    let mut package_rows = Vec::new();
    for manifest in manifests {
        let Some(policy) = policy_rows.get(manifest.name.as_str()) else {
            bail!(
                "internal error: manifest package {:?} missing policy row after validation",
                manifest.name
            );
        };
        let version = packages_by_id
            .values()
            .find(|package| package.name == manifest.name)
            .map(|package| package.version.clone())
            .ok_or_else(|| eyre!("cargo metadata missing version for {}", manifest.name))?;
        package_rows.push(TopologyPackageRow {
            name: manifest.name.clone(),
            version,
            path: manifest.path.clone(),
            publish_disposition: publish_label(manifest.publish_disposition),
            repository_owner: policy.current_owner.clone(),
            integration_mode: policy.current_integration_mode.clone(),
        });
    }
    package_rows.sort_by(|left, right| left.name.cmp(&right.name));

    let declared: BTreeMap<(&str, &str), DeclaredDependency> = metadata
        .packages
        .iter()
        .filter(|package| member_ids.contains(package.id.as_str()))
        .flat_map(|package| {
            package.dependencies.iter().map(move |dependency| {
                ((package.name.as_str(), dependency.name.as_str()), dependency.clone())
            })
        })
        .collect();

    let mut edges = Vec::new();
    for node in &metadata.resolve.nodes {
        let Some(from_package) = packages_by_id.get(node.id.as_str()) else {
            continue;
        };
        for resolved in &node.deps {
            let declared = declared.get(&(from_package.name.as_str(), resolved.name.as_str()));
            let optional = declared.map(|dependency| dependency.optional).unwrap_or(false);
            let rename = declared.and_then(|dependency| dependency.rename.clone());
            let target_pkg = metadata.packages.iter().find(|package| package.id == resolved.pkg);
            let (to_scope, to_path) = match target_pkg {
                Some(package) if member_ids.contains(package.id.as_str()) => {
                    let path = package_path_relative(root, package)?;
                    ("workspace_internal".to_owned(), Some(path))
                }
                Some(_) => ("external".to_owned(), None),
                None => ("unknown".to_owned(), None),
            };
            let to_name = target_pkg
                .map(|package| package.name.clone())
                .unwrap_or_else(|| resolved.name.clone());
            if resolved.dep_kinds.is_empty() {
                edges.push(DirectDependencyEdge {
                    from: from_package.name.clone(),
                    to: to_name,
                    dependency_kind: "normal".to_owned(),
                    optional,
                    rename: rename.clone(),
                    target: None,
                    to_scope,
                    to_path: to_path.clone(),
                });
            } else {
                for kind in &resolved.dep_kinds {
                    edges.push(DirectDependencyEdge {
                        from: from_package.name.clone(),
                        to: to_name.clone(),
                        dependency_kind: kind.kind.clone().unwrap_or_else(|| "normal".to_owned()),
                        optional,
                        rename: rename.clone(),
                        target: kind.target.clone(),
                        to_scope: to_scope.clone(),
                        to_path: to_path.clone(),
                    });
                }
            }
        }
    }

    edges.sort_by(|left, right| {
        (
            &left.from,
            &left.to,
            &left.dependency_kind,
            left.optional,
            &left.rename,
            &left.target,
            &left.to_scope,
            &left.to_path,
        )
            .cmp(&(
                &right.from,
                &right.to,
                &right.dependency_kind,
                right.optional,
                &right.rename,
                &right.target,
                &right.to_scope,
                &right.to_path,
            ))
    });

    let internal_count = edges.iter().filter(|edge| edge.to_scope == "workspace_internal").count();

    Ok(WorkspaceCargoTopology {
        schema_version: SCHEMA.to_owned(),
        generated_by: "cargo xtask repo-topology".to_owned(),
        package_count: package_rows.len(),
        workspace_internal_edge_count: internal_count,
        packages: package_rows,
        direct_dependency_edges: edges,
    })
}

fn load_full_cargo_metadata(root: &Path) -> Result<FullCargoMetadata> {
    let manifest_path = root.join("Cargo.toml");
    let output = Command::new("cargo")
        .arg("metadata")
        .arg("--format-version")
        .arg("1")
        .arg("--locked")
        .arg("--manifest-path")
        .arg(&manifest_path)
        .output()
        .wrap_err("failed to run `cargo metadata`")?;
    if !output.status.success() {
        bail!("cargo metadata failed:\n{}", String::from_utf8_lossy(&output.stderr));
    }
    serde_json::from_slice(&output.stdout).wrap_err("failed to parse cargo metadata")
}

fn package_path_relative(root: &Path, package: &FullCargoPackage) -> Result<String> {
    let dir = Path::new(&package.manifest_path)
        .parent()
        .ok_or_else(|| eyre!("{} has no parent", package.manifest_path))?;
    dir.strip_prefix(root)
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
        .wrap_err_with(|| format!("cargo reported {} outside the repository root", dir.display()))
}

fn publish_label(disposition: PublishDisposition) -> String {
    match disposition {
        PublishDisposition::Published => "published".to_owned(),
        PublishDisposition::PrivateWorkspaceMember => "private_workspace_member".to_owned(),
    }
}

fn render_json(view: &WorkspaceCargoTopology) -> Result<String> {
    Ok(format!("{}\n", serde_json::to_string_pretty(view)?))
}

fn render_markdown(view: &WorkspaceCargoTopology) -> String {
    let mut out = String::new();
    out.push_str("<!-- auto-generated by `cargo xtask repo-topology`; do not edit -->\n\n");
    out.push_str("# Workspace Cargo topology\n\n");
    out.push_str(
        "Deterministic workspace member, publication, and direct-dependency projection (#8412).\n\
Publication disposition and repository ownership come from `policy/repository-topology.toml`;\n\
package versions and dependency edges come from locked `cargo metadata`.\n\n",
    );
    let _ = writeln!(
        out,
        "- Workspace packages: **{}**\n- Workspace-internal direct dependency edges: **{}**\n- Machine-readable inventory: [`workspace-cargo-topology.v1.json`](workspace-cargo-topology.v1.json)\n",
        view.package_count, view.workspace_internal_edge_count
    );

    out.push_str("\n## Workspace packages\n\n");
    out.push_str("| Package | Version | Path | Publish | Repository owner | Integration mode |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- |\n");
    for package in &view.packages {
        let _ = writeln!(
            out,
            "| `{}` | `{}` | `{}` | {} | `{}` | `{}` |",
            package.name,
            package.version,
            package.path,
            package.publish_disposition,
            package.repository_owner,
            package.integration_mode
        );
    }

    out.push_str("\n## Workspace-internal direct dependencies\n\n");
    out.push_str("| From | To | Kind | Optional | Rename | Target cfg | To path |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
    for edge in
        view.direct_dependency_edges.iter().filter(|edge| edge.to_scope == "workspace_internal")
    {
        let rename = edge.rename.as_deref().unwrap_or("—");
        let target = edge.target.as_deref().unwrap_or("—");
        let to_path = edge.to_path.as_deref().unwrap_or("—");
        let _ = writeln!(
            out,
            "| `{}` | `{}` | `{}` | {} | {} | {} | `{}` |",
            edge.from, edge.to, edge.dependency_kind, edge.optional, rename, target, to_path
        );
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::repository_topology::{
        load, read_workspace_excludes, read_workspace_manifests, validate,
    };

    type TestResult<T = ()> = Result<T>;

    #[test]
    fn cargo_topology_matches_workspace_and_policy() -> TestResult {
        let root = project_root()?;
        let topology = load(&root)?;
        let manifests = read_workspace_manifests(&root)?;
        let excludes = read_workspace_excludes(&root)?;
        validate(&topology, &manifests, &excludes)?;
        let view = build_workspace_cargo_topology(&root, &topology, &manifests)?;
        assert_eq!(view.schema_version, SCHEMA);
        assert_eq!(view.packages.len(), manifests.len());
        assert!(view.workspace_internal_edge_count > 0);
        Ok(())
    }

    #[test]
    fn checked_in_cargo_topology_projection_is_current() -> TestResult {
        let root = project_root()?;
        let topology = load(&root)?;
        let manifests = read_workspace_manifests(&root)?;
        write_or_check_cargo_topology(true, &topology, &manifests)
    }

    #[test]
    fn cargo_topology_json_is_deterministic() -> TestResult {
        let root = project_root()?;
        let topology = load(&root)?;
        let manifests = read_workspace_manifests(&root)?;
        let first = build_workspace_cargo_topology(&root, &topology, &manifests)?;
        let second = build_workspace_cargo_topology(&root, &topology, &manifests)?;
        assert_eq!(render_json(&first)?, render_json(&second)?);
        Ok(())
    }
}
