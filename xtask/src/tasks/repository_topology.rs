//! Validate `policy/repository-topology.toml` and project it to a human table.
//!
//! This layer is the data authority for the repository-split programme (#7370).
//! It answers "which repository owns this package, in which migration state, and
//! under which integration mode" without moving source or changing a dependency.
//! Static Cargo/dependency enforcement is #7683, package-isolation execution is
//! #7688, and CI routing plus state promotion is #7695.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

pub const TOPOLOGY_PATH: &str = "policy/repository-topology.toml";
pub const PROJECTION_PATH: &str = "docs/architecture/repository-topology.md";
const SCHEMA: &str = "repository_topology.v1";

/// Integration modes that make a dependency immutable for a consumer.
const IMMUTABLE_MODES: &[&str] = &["exact_git_bridge", "released_registry"];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Topology {
    pub schema_version: String,
    pub migration_states: Vec<MigrationState>,
    pub repositories: Vec<Repository>,
    pub packages: Vec<Package>,
    #[serde(default)]
    pub excluded_trees: Vec<ExcludedTree>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationState {
    pub state: String,
    pub description: String,
    pub legal_integration_modes: Vec<String>,
    pub allows_embedded_workspace_source: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repository {
    pub repository_id: String,
    /// Concrete target repository. Absent only while the identity is undecided,
    /// in which case `target_decision_owner` names the issue that will choose it.
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub target_decision_owner: Option<String>,
    pub migration_state: String,
    pub role: String,
    pub allowed_dependencies: Vec<String>,
    pub forbidden_dependencies: Vec<String>,
    pub history_transfer: String,
    pub controller: String,
    pub move_issue: String,
    pub blocking_prerequisites: Vec<String>,
    #[serde(default)]
    pub authority_refs: Vec<String>,
    pub notes: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub name: String,
    pub path: String,
    pub current_owner: String,
    pub placement: Placement,
    /// Recorded only when `placement` is `accepted`.
    #[serde(default)]
    pub future_owner: Option<String>,
    /// Recorded only when `placement` is `pending`.
    #[serde(default)]
    pub placement_question: Option<String>,
    pub migration_owner: String,
    pub publish_disposition: PublishDisposition,
    pub current_integration_mode: String,
    pub metadata_authority: MetadataAuthority,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    Accepted,
    Pending,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PublishDisposition {
    Published,
    PrivateWorkspaceMember,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MetadataAuthority {
    WorkspaceInherited,
    PackageExplicit,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExcludedTree {
    pub path: String,
    pub kind: String,
    pub owner: String,
    pub notes: String,
}

/// One root-workspace member as the manifests actually describe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestPackage {
    pub name: String,
    pub path: String,
    pub publish_disposition: PublishDisposition,
    pub metadata_authority: MetadataAuthority,
}

pub fn run(check: bool) -> Result<()> {
    let root = project_root()?;
    let topology = load(&root)?;
    let manifests = read_workspace_manifests(&root)?;
    validate(&topology, &manifests)?;

    let projection = render(&topology);
    let projection_path = root.join(PROJECTION_PATH);

    if check {
        let existing = fs::read_to_string(&projection_path)
            .wrap_err_with(|| format!("failed to read {PROJECTION_PATH}"))?;
        if normalize_newlines(&existing) != projection {
            bail!("{PROJECTION_PATH} is stale; run `cargo xtask repo-topology`");
        }
        println!(
            "repository topology is valid and current: {} repositories, {} packages",
            topology.repositories.len(),
            topology.packages.len()
        );
        return Ok(());
    }

    if let Some(parent) = projection_path.parent() {
        fs::create_dir_all(parent)
            .wrap_err_with(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&projection_path, &projection)
        .wrap_err_with(|| format!("failed to write {PROJECTION_PATH}"))?;
    println!(
        "wrote {PROJECTION_PATH} from {} repositories and {} packages",
        topology.repositories.len(),
        topology.packages.len()
    );
    Ok(())
}

fn load(root: &Path) -> Result<Topology> {
    let path = root.join(TOPOLOGY_PATH);
    let text =
        fs::read_to_string(&path).wrap_err_with(|| format!("failed to read {TOPOLOGY_PATH}"))?;
    parse(&text)
}

pub fn parse(text: &str) -> Result<Topology> {
    toml::from_str(text).wrap_err_with(|| format!("failed to parse {TOPOLOGY_PATH}"))
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// An issue reference must be `#` followed by digits so downstream tooling can
/// resolve it without guessing at prose.
fn is_issue_ref(value: &str) -> bool {
    let Some(digits) = value.strip_prefix('#') else {
        return false;
    };
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn check_issue_ref(errors: &mut Vec<String>, subject: &str, field: &str, value: &str) {
    if !is_issue_ref(value) {
        errors
            .push(format!("{subject}: {field} {value:?} is not an issue reference like \"#123\""));
    }
}

pub fn validate(topology: &Topology, manifests: &[ManifestPackage]) -> Result<()> {
    let mut errors = Vec::new();

    if topology.schema_version != SCHEMA {
        bail!("unsupported schema_version {:?}; expected {:?}", topology.schema_version, SCHEMA);
    }
    if topology.migration_states.is_empty() {
        bail!("repository topology must declare at least one migration state");
    }
    if topology.repositories.is_empty() {
        bail!("repository topology must declare at least one repository");
    }

    // Migration states.
    let mut states: BTreeMap<&str, &MigrationState> = BTreeMap::new();
    for state in &topology.migration_states {
        if states.insert(state.state.as_str(), state).is_some() {
            errors.push(format!("duplicate migration state {:?}", state.state));
        }
        if state.legal_integration_modes.is_empty() {
            errors.push(format!(
                "migration state {:?} declares no legal integration modes",
                state.state
            ));
        }
        let mut seen_modes = BTreeSet::new();
        for mode in &state.legal_integration_modes {
            if !seen_modes.insert(mode.as_str()) {
                errors.push(format!(
                    "migration state {:?} repeats integration mode {mode:?}",
                    state.state
                ));
            }
        }
    }

    // Repositories.
    let mut repository_ids = BTreeSet::new();
    for repo in &topology.repositories {
        let subject = format!("repository {:?}", repo.repository_id);
        if !repository_ids.insert(repo.repository_id.as_str()) {
            errors.push(format!("duplicate repository id {:?}", repo.repository_id));
        }
        if !states.contains_key(repo.migration_state.as_str()) {
            errors.push(format!("{subject}: unknown migration_state {:?}", repo.migration_state));
        }
        match (&repo.target, &repo.target_decision_owner) {
            (Some(target), None) => {
                if target.trim().is_empty() {
                    errors.push(format!("{subject}: empty target"));
                }
            }
            (None, Some(owner)) => {
                check_issue_ref(&mut errors, &subject, "target_decision_owner", owner)
            }
            (Some(_), Some(_)) => {
                errors.push(format!("{subject}: set target or target_decision_owner, not both"))
            }
            (None, None) => {
                errors.push(format!("{subject}: must set target or target_decision_owner"))
            }
        }
        // A repository that has actually left cannot still have an undecided identity.
        if matches!(repo.migration_state.as_str(), "externalizing" | "external")
            && repo.target.is_none()
        {
            errors.push(format!(
                "{subject}: state {:?} requires a concrete target",
                repo.migration_state
            ));
        }
        check_issue_ref(&mut errors, &subject, "controller", &repo.controller);
        check_issue_ref(&mut errors, &subject, "move_issue", &repo.move_issue);
        for value in repo.blocking_prerequisites.iter().chain(repo.authority_refs.iter()) {
            check_issue_ref(&mut errors, &subject, "issue reference", value);
        }
        if repo.notes.trim().is_empty() {
            errors.push(format!("{subject}: empty notes"));
        }
    }

    // Dependency edges resolve, do not self-reference, and do not contradict.
    for repo in &topology.repositories {
        let subject = format!("repository {:?}", repo.repository_id);
        let allowed: BTreeSet<&str> =
            repo.allowed_dependencies.iter().map(String::as_str).collect();
        let forbidden: BTreeSet<&str> =
            repo.forbidden_dependencies.iter().map(String::as_str).collect();
        for edge in allowed.iter().chain(forbidden.iter()) {
            if !repository_ids.contains(edge) {
                errors.push(format!("{subject}: unknown dependency target {edge:?}"));
            }
            if *edge == repo.repository_id.as_str() {
                errors.push(format!("{subject}: declares a dependency on itself"));
            }
        }
        for edge in allowed.intersection(&forbidden) {
            errors.push(format!("{subject}: {edge:?} is both allowed and forbidden"));
        }
    }

    // Packages.
    let mut package_names = BTreeSet::new();
    let mut previous: Option<&str> = None;
    for package in &topology.packages {
        let subject = format!("package {:?}", package.name);
        if !package_names.insert(package.name.as_str()) {
            errors.push(format!("duplicate package row {:?}", package.name));
        }
        // Canonical order keeps diffs and the projection deterministic.
        if let Some(prev) = previous
            && prev >= package.name.as_str()
        {
            errors.push(format!(
                "package rows must be sorted by name: {:?} follows {prev:?}",
                package.name
            ));
        }
        previous = Some(package.name.as_str());

        if !repository_ids.contains(package.current_owner.as_str()) {
            errors.push(format!("{subject}: unknown current_owner {:?}", package.current_owner));
        }
        check_issue_ref(&mut errors, &subject, "migration_owner", &package.migration_owner);

        match package.placement {
            Placement::Accepted => {
                match package.future_owner.as_deref() {
                    Some(owner) if repository_ids.contains(owner) => {}
                    Some(owner) => {
                        errors.push(format!("{subject}: unknown future_owner {owner:?}"))
                    }
                    None => {
                        errors.push(format!("{subject}: accepted placement requires future_owner"))
                    }
                }
                if package.placement_question.is_some() {
                    errors.push(format!(
                        "{subject}: accepted placement must not carry placement_question"
                    ));
                }
            }
            Placement::Pending => {
                if package.future_owner.is_some() {
                    errors
                        .push(format!("{subject}: pending placement must not assert future_owner"));
                }
                if package.placement_question.as_deref().is_none_or(|q| q.trim().is_empty()) {
                    errors.push(format!(
                        "{subject}: pending placement requires a non-empty placement_question"
                    ));
                }
            }
        }

        // The integration mode must be legal for the state its current owner is in.
        if let Some(owner) =
            topology.repositories.iter().find(|repo| repo.repository_id == package.current_owner)
            && let Some(state) = states.get(owner.migration_state.as_str())
        {
            if !state.legal_integration_modes.contains(&package.current_integration_mode) {
                errors.push(format!(
                    "{subject}: integration mode {:?} is not legal for owner state {:?}",
                    package.current_integration_mode, owner.migration_state
                ));
            }
            if !state.allows_embedded_workspace_source
                && package.current_integration_mode == "workspace_path"
            {
                errors.push(format!(
                    "{subject}: owner state {:?} forbids embedded workspace source",
                    owner.migration_state
                ));
            }
            // Anything consumed across a real repository boundary must be immutable.
            if !state.allows_embedded_workspace_source
                && !IMMUTABLE_MODES.contains(&package.current_integration_mode.as_str())
            {
                errors.push(format!(
                    "{subject}: owner state {:?} requires an immutable consumption mode",
                    owner.migration_state
                ));
            }
        }
    }

    // Excluded trees.
    let mut excluded_paths = BTreeSet::new();
    for tree in &topology.excluded_trees {
        let subject = format!("excluded tree {:?}", tree.path);
        if !excluded_paths.insert(tree.path.as_str()) {
            errors.push(format!("duplicate excluded tree {:?}", tree.path));
        }
        check_issue_ref(&mut errors, &subject, "owner", &tree.owner);
    }

    errors.extend(reconcile_with_manifests(topology, manifests));

    if !errors.is_empty() {
        let mut report = format!("{TOPOLOGY_PATH} is invalid ({} finding(s)):", errors.len());
        for error in &errors {
            report.push_str("\n  - ");
            report.push_str(error);
        }
        bail!(report);
    }
    Ok(())
}

/// The topology must describe the workspace that actually exists: no missing
/// package, no invented package, and no stale publish/metadata classification.
fn reconcile_with_manifests(topology: &Topology, manifests: &[ManifestPackage]) -> Vec<String> {
    let mut errors = Vec::new();
    let rows: BTreeMap<&str, &Package> =
        topology.packages.iter().map(|package| (package.name.as_str(), package)).collect();
    let actual: BTreeMap<&str, &ManifestPackage> =
        manifests.iter().map(|package| (package.name.as_str(), package)).collect();

    for (name, manifest) in &actual {
        let Some(row) = rows.get(name) else {
            errors.push(format!(
                "workspace member {name:?} has no repository-topology row; classify it or it stays unowned"
            ));
            continue;
        };
        if row.path != manifest.path {
            errors.push(format!(
                "package {name:?}: path {:?} does not match workspace member path {:?}",
                row.path, manifest.path
            ));
        }
        if row.publish_disposition != manifest.publish_disposition {
            errors.push(format!(
                "package {name:?}: publish_disposition {:?} contradicts its manifest",
                row.publish_disposition
            ));
        }
        if row.metadata_authority != manifest.metadata_authority {
            errors.push(format!(
                "package {name:?}: metadata_authority {:?} contradicts its manifest",
                row.metadata_authority
            ));
        }
    }
    for name in rows.keys() {
        if !actual.contains_key(name) {
            errors.push(format!("repository-topology row {name:?} is not a root-workspace member"));
        }
    }
    errors
}

/// Read the root workspace members and classify each one from its own manifest.
pub fn read_workspace_manifests(root: &Path) -> Result<Vec<ManifestPackage>> {
    let manifest_path = root.join("Cargo.toml");
    let text = fs::read_to_string(&manifest_path)
        .wrap_err_with(|| format!("failed to read {}", manifest_path.display()))?;
    let manifest: toml::Value = toml::from_str(&text)
        .wrap_err_with(|| format!("failed to parse {}", manifest_path.display()))?;
    let members = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(toml::Value::as_array)
        .ok_or_else(|| color_eyre::eyre::eyre!("root Cargo.toml has no [workspace] members"))?;

    let mut packages = Vec::new();
    for member in members {
        let Some(member) = member.as_str() else {
            bail!("workspace member entries must be strings");
        };
        let member_manifest = root.join(member).join("Cargo.toml");
        let member_text = fs::read_to_string(&member_manifest)
            .wrap_err_with(|| format!("failed to read {}", member_manifest.display()))?;
        let parsed: toml::Value = toml::from_str(&member_text)
            .wrap_err_with(|| format!("failed to parse {}", member_manifest.display()))?;
        let package = parsed
            .get("package")
            .ok_or_else(|| color_eyre::eyre::eyre!("{member} has no [package] table"))?;
        let name = package
            .get("name")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| color_eyre::eyre::eyre!("{member} has no package name"))?;
        // `publish = false` is the only way a member opts out of publication;
        // any other shape (absent, `true`, a registry list) stays publishable.
        let published = package.get("publish").and_then(toml::Value::as_bool) != Some(false);
        let explicit_metadata = package.get("repository").and_then(toml::Value::as_str).is_some();
        packages.push(ManifestPackage {
            name: name.to_string(),
            path: member.to_string(),
            publish_disposition: if published {
                PublishDisposition::Published
            } else {
                PublishDisposition::PrivateWorkspaceMember
            },
            metadata_authority: if explicit_metadata {
                MetadataAuthority::PackageExplicit
            } else {
                MetadataAuthority::WorkspaceInherited
            },
        });
    }
    packages.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(packages)
}

fn placement_label(package: &Package) -> String {
    match package.placement {
        Placement::Accepted => package.future_owner.clone().unwrap_or_else(|| "—".to_string()),
        Placement::Pending => "_pending_".to_string(),
    }
}

pub fn render(topology: &Topology) -> String {
    let mut out = String::new();
    out.push_str("<!-- auto-generated by `cargo xtask repo-topology`; do not edit -->\n\n");
    out.push_str("# Repository topology and package ownership\n\n");
    out.push_str(
        "Generated from `policy/repository-topology.toml`. That file is the authority; this page is a\nprojection of it. Edit the policy file and run `cargo xtask repo-topology`.\n\n",
    );
    out.push_str(
        "Controller #7370, programme #7369. This page records ownership and migration state only.\nStatic dependency/metadata enforcement is #7683, isolation execution is #7688, and CI routing\nis #7695.\n\n",
    );

    out.push_str("## Migration states\n\n");
    out.push_str("| State | Embedded source | Legal integration modes | Meaning |\n");
    out.push_str("| --- | --- | --- | --- |\n");
    for state in &topology.migration_states {
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} |",
            state.state,
            if state.allows_embedded_workspace_source { "yes" } else { "no" },
            state
                .legal_integration_modes
                .iter()
                .map(|mode| format!("`{mode}`"))
                .collect::<Vec<_>>()
                .join(", "),
            state.description
        );
    }

    out.push_str("\n## Repositories\n\n");
    out.push_str(
        "| Repository | Target | State | Role | History transfer | Controller | Move issue | Blocked by |\n",
    );
    out.push_str("| --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for repo in &topology.repositories {
        let target = match (&repo.target, &repo.target_decision_owner) {
            (Some(target), _) => format!("`{target}`"),
            (None, Some(owner)) => format!("_undecided, {owner}_"),
            (None, None) => "_unset_".to_string(),
        };
        let blocked = if repo.blocking_prerequisites.is_empty() {
            "—".to_string()
        } else {
            repo.blocking_prerequisites.join(", ")
        };
        let _ = writeln!(
            out,
            "| `{}` | {} | `{}` | `{}` | `{}` | {} | {} | {} |",
            repo.repository_id,
            target,
            repo.migration_state,
            repo.role,
            repo.history_transfer,
            repo.controller,
            repo.move_issue,
            blocked
        );
    }

    out.push_str("\n## Package ownership\n\n");
    for repo in &topology.repositories {
        let owned: Vec<&Package> = topology
            .packages
            .iter()
            .filter(|package| package.current_owner == repo.repository_id)
            .collect();
        let _ = writeln!(out, "### `{}`\n", repo.repository_id);
        let _ = writeln!(out, "{}\n", repo.notes);
        if owned.is_empty() {
            out.push_str("_No package currently lives here._\n\n");
            continue;
        }
        out.push_str("| Package | Future owner | Publish | Integration mode | Owner issue |\n");
        out.push_str("| --- | --- | --- | --- | --- |\n");
        for package in owned {
            let publish = match package.publish_disposition {
                PublishDisposition::Published => "published",
                PublishDisposition::PrivateWorkspaceMember => "private",
            };
            let _ = writeln!(
                out,
                "| `{}` | {} | {} | `{}` | {} |",
                package.name,
                placement_label(package),
                publish,
                package.current_integration_mode,
                package.migration_owner
            );
        }
        out.push('\n');
    }

    let pending: Vec<&Package> = topology
        .packages
        .iter()
        .filter(|package| package.placement == Placement::Pending)
        .collect();
    out.push_str("## Unresolved placement\n\n");
    if pending.is_empty() {
        out.push_str("_Every package has an accepted future owner._\n");
    } else {
        let _ = writeln!(
            out,
            "{} package(s) have no accepted future owner. Each names the issue that owns the\ndecision and the exact question, so unresolved placement stays visible instead of\nbecoming a false final answer.\n",
            pending.len()
        );
        out.push_str("| Package | Owner issue | Open question |\n");
        out.push_str("| --- | --- | --- |\n");
        for package in pending {
            let _ = writeln!(
                out,
                "| `{}` | {} | {} |",
                package.name,
                package.migration_owner,
                package.placement_question.as_deref().unwrap_or("")
            );
        }
    }

    if !topology.excluded_trees.is_empty() {
        out.push_str("\n## Excluded source trees\n\n");
        out.push_str("| Path | Kind | Owner | Notes |\n");
        out.push_str("| --- | --- | --- | --- |\n");
        for tree in &topology.excluded_trees {
            let _ = writeln!(
                out,
                "| `{}` | `{}` | {} | {} |",
                tree.path, tree.kind, tree.owner, tree.notes
            );
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult<T = ()> = Result<T>;

    /// A minimal well-formed topology the negative controls mutate one field at a time.
    fn fixture() -> String {
        r##"
schema_version = "repository_topology.v1"

[[migration_states]]
state = "embedded"
description = "Source lives here."
legal_integration_modes = ["workspace_path"]
allows_embedded_workspace_source = true

[[migration_states]]
state = "external"
description = "Owned elsewhere."
legal_integration_modes = ["released_registry"]
allows_embedded_workspace_source = false

[[repositories]]
repository_id = "product"
target = "Org/product"
migration_state = "embedded"
role = "product_integration"
allowed_dependencies = ["library"]
forbidden_dependencies = []
history_transfer = "not_applicable"
controller = "#1"
move_issue = "#2"
blocking_prerequisites = []
authority_refs = []
notes = "The product repository."

[[repositories]]
repository_id = "library"
target = "Org/library"
migration_state = "embedded"
role = "library"
allowed_dependencies = []
forbidden_dependencies = ["product"]
history_transfer = "preserve_path_history"
controller = "#3"
move_issue = "#4"
blocking_prerequisites = ["#5"]
authority_refs = []
notes = "A library being prepared."

[[packages]]
name = "alpha"
path = "crates/alpha"
current_owner = "product"
placement = "accepted"
future_owner = "library"
migration_owner = "#3"
publish_disposition = "published"
current_integration_mode = "workspace_path"
metadata_authority = "workspace_inherited"

[[packages]]
name = "beta"
path = "crates/beta"
current_owner = "product"
placement = "pending"
migration_owner = "#6"
placement_question = "Undecided."
publish_disposition = "private_workspace_member"
current_integration_mode = "workspace_path"
metadata_authority = "workspace_inherited"
"##
        .to_string()
    }

    fn manifests() -> Vec<ManifestPackage> {
        vec![
            ManifestPackage {
                name: "alpha".into(),
                path: "crates/alpha".into(),
                publish_disposition: PublishDisposition::Published,
                metadata_authority: MetadataAuthority::WorkspaceInherited,
            },
            ManifestPackage {
                name: "beta".into(),
                path: "crates/beta".into(),
                publish_disposition: PublishDisposition::PrivateWorkspaceMember,
                metadata_authority: MetadataAuthority::WorkspaceInherited,
            },
        ]
    }

    /// Assert the topology is rejected for the stated reason, so a negative control
    /// cannot pass by failing for some unrelated reason.
    fn expect_rejected(source: &str, needle: &str) -> TestResult {
        let topology = parse(source)?;
        let error = match validate(&topology, &manifests()) {
            Ok(()) => bail!("expected validation to reject: {needle}"),
            Err(error) => error.to_string(),
        };
        if !error.contains(needle) {
            bail!("expected error containing {needle:?}, got:\n{error}");
        }
        Ok(())
    }

    fn expect_rejected_against(
        topology: &Topology,
        actual: &[ManifestPackage],
        needle: &str,
    ) -> TestResult {
        let error = match validate(topology, actual) {
            Ok(()) => bail!("expected validation to reject: {needle}"),
            Err(error) => error.to_string(),
        };
        if !error.contains(needle) {
            bail!("expected error containing {needle:?}, got:\n{error}");
        }
        Ok(())
    }

    #[test]
    fn accepts_the_fixture() -> TestResult {
        let topology = parse(&fixture())?;
        validate(&topology, &manifests())?;
        Ok(())
    }

    #[test]
    fn rejects_unsupported_schema() -> TestResult {
        let source = fixture().replace("repository_topology.v1", "repository_topology.v2");
        expect_rejected(&source, "unsupported schema_version")
    }

    #[test]
    fn rejects_duplicate_repository_id() -> TestResult {
        let source =
            fixture().replace(r#"repository_id = "library""#, r#"repository_id = "product""#);
        expect_rejected(&source, "duplicate repository id")
    }

    #[test]
    fn rejects_duplicate_package_row() -> TestResult {
        let source = fixture().replace(r#"name = "beta""#, r#"name = "alpha""#);
        expect_rejected(&source, "duplicate package row")
    }

    #[test]
    fn rejects_unsorted_package_rows() -> TestResult {
        let source = fixture()
            .replace(r#"name = "alpha""#, r#"name = "zeta""#)
            .replace(r#"path = "crates/alpha""#, r#"path = "crates/zeta""#);
        // `zeta` now precedes `beta`, so canonical ordering fails.
        expect_rejected(&source, "must be sorted by name")
    }

    #[test]
    fn rejects_unknown_current_owner() -> TestResult {
        let source =
            fixture().replace(r#"current_owner = "product""#, r#"current_owner = "ghost""#);
        expect_rejected(&source, "unknown current_owner")
    }

    #[test]
    fn rejects_pending_placement_without_a_question() -> TestResult {
        let source = fixture().replace("placement_question = \"Undecided.\"\n", "");
        expect_rejected(&source, "requires a non-empty placement_question")
    }

    #[test]
    fn rejects_pending_placement_that_asserts_a_future_owner() -> TestResult {
        let source = fixture().replace(
            "placement = \"pending\"\nmigration_owner = \"#6\"",
            "placement = \"pending\"\nfuture_owner = \"library\"\nmigration_owner = \"#6\"",
        );
        expect_rejected(&source, "must not assert future_owner")
    }

    #[test]
    fn rejects_accepted_placement_without_a_future_owner() -> TestResult {
        let source = fixture().replace("future_owner = \"library\"\n", "");
        expect_rejected(&source, "accepted placement requires future_owner")
    }

    #[test]
    fn rejects_contradictory_dependency_edge() -> TestResult {
        // `product` already allows `library`; forbidding it too is the contradiction.
        let source = fixture()
            .replace("forbidden_dependencies = []", r#"forbidden_dependencies = ["library"]"#);
        expect_rejected(&source, "is both allowed and forbidden")
    }

    #[test]
    fn rejects_unknown_dependency_target() -> TestResult {
        let source = fixture().replace(
            r#"allowed_dependencies = ["library"]"#,
            r#"allowed_dependencies = ["ghost"]"#,
        );
        expect_rejected(&source, "unknown dependency target")
    }

    #[test]
    fn rejects_non_issue_reference() -> TestResult {
        let source = fixture().replace("controller = \"#1\"", "controller = \"owner-team\"");
        expect_rejected(&source, "is not an issue reference")
    }

    #[test]
    fn rejects_external_state_without_a_concrete_target() -> TestResult {
        let source = fixture().replace(
            "repository_id = \"library\"\ntarget = \"Org/library\"\nmigration_state = \"embedded\"",
            "repository_id = \"library\"\ntarget_decision_owner = \"#9\"\nmigration_state = \"external\"",
        );
        expect_rejected(&source, "requires a concrete target")
    }

    #[test]
    fn rejects_embedded_source_under_an_external_owner() -> TestResult {
        let source = fixture().replace(
            "repository_id = \"product\"\ntarget = \"Org/product\"\nmigration_state = \"embedded\"",
            "repository_id = \"product\"\ntarget = \"Org/product\"\nmigration_state = \"external\"",
        );
        expect_rejected(&source, "forbids embedded workspace source")
    }

    #[test]
    fn rejects_target_and_decision_owner_together() -> TestResult {
        let source = fixture().replace(
            "target = \"Org/library\"",
            "target = \"Org/library\"\ntarget_decision_owner = \"#9\"",
        );
        expect_rejected(&source, "not both")
    }

    #[test]
    fn rejects_a_workspace_member_with_no_row() -> TestResult {
        let topology = parse(&fixture())?;
        let mut actual = manifests();
        actual.push(ManifestPackage {
            name: "gamma".into(),
            path: "crates/gamma".into(),
            publish_disposition: PublishDisposition::Published,
            metadata_authority: MetadataAuthority::WorkspaceInherited,
        });
        expect_rejected_against(&topology, &actual, "has no repository-topology row")
    }

    #[test]
    fn rejects_a_row_that_is_not_a_workspace_member() -> TestResult {
        let topology = parse(&fixture())?;
        let actual = vec![manifests()[0].clone()];
        expect_rejected_against(&topology, &actual, "is not a root-workspace member")
    }

    #[test]
    fn rejects_a_stale_publish_disposition() -> TestResult {
        let topology = parse(&fixture())?;
        let mut actual = manifests();
        actual[0].publish_disposition = PublishDisposition::PrivateWorkspaceMember;
        expect_rejected_against(&topology, &actual, "contradicts its manifest")
    }

    #[test]
    fn rejects_a_stale_metadata_authority() -> TestResult {
        let topology = parse(&fixture())?;
        let mut actual = manifests();
        actual[0].metadata_authority = MetadataAuthority::PackageExplicit;
        expect_rejected_against(&topology, &actual, "contradicts its manifest")
    }

    #[test]
    fn rejects_unknown_fields() {
        let source = format!("{}\nunexpected_key = true\n", fixture());
        assert!(parse(&source).is_err(), "unknown top-level keys must be rejected");
    }

    /// The seeded authority must describe the workspace that actually exists. This is
    /// the regression guard: adding a crate without classifying it fails here.
    #[test]
    fn checked_in_topology_matches_the_real_workspace() -> TestResult {
        let root = project_root()?;
        let topology = load(&root)?;
        let manifests = read_workspace_manifests(&root)?;
        validate(&topology, &manifests)?;
        Ok(())
    }

    #[test]
    fn checked_in_projection_is_current() -> TestResult {
        let root = project_root()?;
        let topology = load(&root)?;
        let committed = fs::read_to_string(root.join(PROJECTION_PATH))
            .wrap_err_with(|| format!("failed to read {PROJECTION_PATH}"))?;
        if normalize_newlines(&committed) != render(&topology) {
            bail!("{PROJECTION_PATH} is stale; run `cargo xtask repo-topology`");
        }
        Ok(())
    }

    #[test]
    fn projection_is_deterministic() -> TestResult {
        let topology = parse(&fixture())?;
        assert_eq!(render(&topology), render(&topology));
        Ok(())
    }

    #[test]
    fn projection_reports_pending_placement() -> TestResult {
        let topology = parse(&fixture())?;
        let rendered = render(&topology);
        assert!(rendered.contains("## Unresolved placement"));
        assert!(rendered.contains("`beta`"));
        assert!(rendered.contains("_pending_"));
        Ok(())
    }
}
