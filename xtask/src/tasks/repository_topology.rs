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
use std::path::{Component, Path};
use std::process::Command;

pub const TOPOLOGY_PATH: &str = "policy/repository-topology.toml";
pub const PROJECTION_PATH: &str = "docs/architecture/repository-topology.md";
const SCHEMA: &str = "repository_topology.v1";

/// Integration modes that make a dependency immutable for a consumer.
const IMMUTABLE_MODES: &[&str] = &["exact_git_bridge", "released_registry"];
/// The one mode that means "still compiled from source in this workspace".
const WORKSPACE_PATH_MODE: &str = "workspace_path";

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
    /// Whether a package owned by a repository in this state is still compiled
    /// from source in this workspace.
    pub allows_embedded_workspace_source: bool,
    /// Whether consumption across this boundary must be immutable. This is
    /// declared, not inferred from `allows_embedded_workspace_source`: `retired`
    /// also has no embedded source but is consumed through nothing at all, so
    /// inferring the requirement would make that state unsatisfiable.
    pub requires_immutable_consumption: bool,
    /// Whether a package may still be routed here. Declared for the same reason
    /// as the flag above: a retired repository is not a destination, and deriving
    /// that from the other two flags would be an implicit rule nobody can read.
    pub accepts_future_packages: bool,
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
    pub role: RepositoryRole,
    pub allowed_dependencies: Vec<String>,
    pub forbidden_dependencies: Vec<String>,
    pub history_transfer: HistoryTransfer,
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

/// What a repository is for. A closed vocabulary, like the other classification
/// fields: an unrecognised role in an authority whose whole job is machine-checked
/// classification would otherwise pass validation and land in the generated page.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryRole {
    ProductIntegration,
    ExperimentalParser,
    HistoricalComparisonSubject,
    GenericLspFramework,
    NativeParserWorkspace,
    CorpusAssets,
    LowerSourceIdentity,
}

/// What happens to a repository's history when its source moves.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HistoryTransfer {
    PreservePathHistory,
    NotApplicable,
    Undecided,
}

/// What an excluded tree is.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExcludedTreeKind {
    VendoredUpstreamSource,
    SeparateWorkspace,
}

/// Render a `snake_case`-serialised classification back for the projection.
fn snake_case_name(value: &impl std::fmt::Debug) -> String {
    let camel = format!("{value:?}");
    let mut out = String::with_capacity(camel.len() + 4);
    for (index, ch) in camel.char_indices() {
        if ch.is_ascii_uppercase() {
            if index != 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
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
    /// The manifest sets no `repository` at all, so the package advertises none.
    ///
    /// Distinct from `workspace_inherited`: a package that omits the key inherits
    /// nothing, and recording it as inherited would assert a repository Cargo never
    /// resolved for it.
    Unset,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExcludedTree {
    pub path: String,
    pub kind: ExcludedTreeKind,
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
    let excludes = read_workspace_excludes(&root)?;
    validate(&topology, &manifests, &excludes)?;

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
    // Issue numbering starts at 1, and a leading zero would give one issue two
    // spellings, so neither can resolve to a single real issue.
    !digits.is_empty()
        && !digits.starts_with('0')
        && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// A repository-relative package directory: non-empty, normalized, and inside
/// the tree. Checked independently of workspace reconciliation, because a package
/// whose owner has externalized is never reconciled against a manifest and would
/// otherwise be free to carry a blank or escaping path.
fn check_relative_path(errors: &mut Vec<String>, subject: &str, field: &str, value: &str) {
    let path = Path::new(value);
    if value.trim().is_empty() {
        errors.push(format!("{subject}: {field} is empty"));
        return;
    }
    if value.contains('\\')
        || path.is_absolute()
        || path.components().any(|part| matches!(part, Component::ParentDir | Component::RootDir))
    {
        errors.push(format!(
            "{subject}: {field} {value:?} must be a repository-relative path inside the tree"
        ));
        return;
    }
    // Canonical form, not merely a safe one. `Path::components` silently folds
    // away `./`, `//` and trailing slashes, so a row can carry a spelling that
    // never equals the manifest path it is compared against — and for a package
    // whose owner has externalized, nothing compares it at all.
    let canonical = path
        .components()
        .filter_map(|part| match part {
            Component::Normal(segment) => segment.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    if canonical != value {
        errors.push(format!(
            "{subject}: {field} {value:?} is not canonical; write it as {canonical:?}"
        ));
    }
}

fn check_non_empty(errors: &mut Vec<String>, subject: &str, field: &str, value: &str) {
    if value.trim().is_empty() {
        errors.push(format!("{subject}: {field} is empty"));
    }
}

fn check_issue_ref(errors: &mut Vec<String>, subject: &str, field: &str, value: &str) {
    if !is_issue_ref(value) {
        errors
            .push(format!("{subject}: {field} {value:?} is not an issue reference like \"#123\""));
    }
}

pub fn validate(
    topology: &Topology,
    manifests: &[ManifestPackage],
    workspace_excludes: &BTreeSet<String>,
) -> Result<()> {
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
        // A blank name or description would give repositories a state to point at
        // whose meaning is unwritten — internally consistent, and unreadable.
        let state_subject = format!("migration state {:?}", state.state);
        check_non_empty(&mut errors, &state_subject, "state", &state.state);
        check_non_empty(&mut errors, &state_subject, "description", &state.description);
        if state.legal_integration_modes.is_empty() {
            errors.push(format!(
                "migration state {:?} declares no legal integration modes",
                state.state
            ));
        }
        let mut seen_modes = BTreeSet::new();
        for mode in &state.legal_integration_modes {
            check_non_empty(&mut errors, &state_subject, "legal_integration_modes entry", mode);
            if !seen_modes.insert(mode.as_str()) {
                errors.push(format!(
                    "migration state {:?} repeats integration mode {mode:?}",
                    state.state
                ));
            }
        }
        // A state must be satisfiable by at least one of its own declared modes.
        // Without this, a state can silently become a trap that rejects every
        // package assigned to it.
        let allows_workspace_path =
            state.legal_integration_modes.iter().any(|mode| mode == WORKSPACE_PATH_MODE);
        // Both directions, or a state becomes a trap no package can satisfy: one
        // that keeps embedded source must permit the mode that expresses it, and
        // one that has given up embedded source must not.
        if !state.allows_embedded_workspace_source && allows_workspace_path {
            errors.push(format!(
                "migration state {:?} forbids embedded source but lists {WORKSPACE_PATH_MODE:?} as legal",
                state.state
            ));
        }
        if state.allows_embedded_workspace_source && !allows_workspace_path {
            errors.push(format!(
                "migration state {:?} keeps embedded source but does not list {WORKSPACE_PATH_MODE:?} as legal",
                state.state
            ));
        }
        if state.requires_immutable_consumption
            && let Some(mode) = state
                .legal_integration_modes
                .iter()
                .find(|mode| !IMMUTABLE_MODES.contains(&mode.as_str()))
        {
            errors.push(format!(
                "migration state {:?} requires immutable consumption but lists mutable mode {mode:?}",
                state.state
            ));
        }
    }

    // Repositories.
    let mut repository_ids = BTreeSet::new();
    for repo in &topology.repositories {
        let subject = format!("repository {:?}", repo.repository_id);
        check_non_empty(&mut errors, &subject, "repository_id", &repo.repository_id);
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
        // A repository that has actually left cannot still have an undecided identity:
        // consumers pin or publish against it, and neither can name a decision. Keyed on
        // the state's declared requirement rather than a hard-coded list of state names,
        // for the same reason that requirement is declared at all — a state added to the
        // policy file must not quietly escape a rule by not being named in Rust.
        if states
            .get(repo.migration_state.as_str())
            .is_some_and(|state| state.requires_immutable_consumption)
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
        // Collapsing into sets would otherwise hide a copy-paste duplicate that
        // is masking a missing distinct target.
        if allowed.len() != repo.allowed_dependencies.len() {
            errors.push(format!("{subject}: allowed_dependencies contains a duplicate"));
        }
        if forbidden.len() != repo.forbidden_dependencies.len() {
            errors.push(format!("{subject}: forbidden_dependencies contains a duplicate"));
        }
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
    let mut package_paths = BTreeSet::new();
    let mut previous: Option<&str> = None;
    for package in &topology.packages {
        let subject = format!("package {:?}", package.name);
        // Identity is checked here, not only through manifest reconciliation: a
        // package owned by an externalized repository is never reconciled, so its
        // row is the sole record of what it is and where it came from.
        check_non_empty(&mut errors, &subject, "name", &package.name);
        check_relative_path(&mut errors, &subject, "path", &package.path);
        if !package_names.insert(package.name.as_str()) {
            errors.push(format!("duplicate package row {:?}", package.name));
        }
        // Two rows claiming one path is a contradiction the manifests cannot catch
        // once the owner has externalized, because neither row is reconciled then.
        if !package_paths.insert(package.path.as_str()) {
            errors.push(format!(
                "package {:?}: path {:?} is already claimed by another package row",
                package.name, package.path
            ));
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
                    Some(owner) if repository_ids.contains(owner) => {
                        // A repository nothing consumes any more is not a destination.
                        if let Some(destination) =
                            topology.repositories.iter().find(|repo| repo.repository_id == owner)
                            && let Some(state) = states.get(destination.migration_state.as_str())
                            && !state.accepts_future_packages
                        {
                            errors.push(format!(
                                "{subject}: future_owner {owner:?} is in state {:?}, which accepts no packages",
                                destination.migration_state
                            ));
                        }
                    }
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
                && package.current_integration_mode == WORKSPACE_PATH_MODE
            {
                errors.push(format!(
                    "{subject}: owner state {:?} forbids embedded workspace source",
                    owner.migration_state
                ));
            }
            // Anything consumed across a real repository boundary must be immutable.
            if state.requires_immutable_consumption
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
        // Held to the same shape rule as a package path: this row asserts that a
        // tree inside the repository is deliberately not a workspace member, and a
        // path that escapes the tree would have the authority govern files that are
        // not ours to govern.
        check_relative_path(&mut errors, &subject, "path", &tree.path);
        check_non_empty(&mut errors, &subject, "notes", &tree.notes);
        if !excluded_paths.insert(tree.path.as_str()) {
            errors.push(format!("duplicate excluded tree {:?}", tree.path));
        }
        check_issue_ref(&mut errors, &subject, "owner", &tree.owner);
        // The rows here are a curated subset of `[workspace] exclude`, but a row
        // naming a tree the workspace no longer excludes is stale, not curated.
        if !workspace_excludes.contains(&tree.path) {
            errors.push(format!(
                "{subject}: not listed in the root workspace `exclude`, so this row is stale"
            ));
        }
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
    let embedded_states: BTreeMap<&str, bool> = topology
        .migration_states
        .iter()
        .map(|state| (state.state.as_str(), state.allows_embedded_workspace_source))
        .collect();
    let owner_states: BTreeMap<&str, &str> = topology
        .repositories
        .iter()
        .map(|repo| (repo.repository_id.as_str(), repo.migration_state.as_str()))
        .collect();
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
        if expects_embedded_source(row, &owner_states, &embedded_states) == Some(false) {
            errors.push(format!(
                "package {name:?}: owner {:?} has taken its source out, but it is still a root-workspace member",
                row.current_owner
            ));
        }
    }
    for (name, row) in &rows {
        if actual.contains_key(name) {
            continue;
        }
        // A legitimately external package is not a workspace member, and its row
        // is the only surviving record of who owns it and how it is consumed.
        if expects_embedded_source(row, &owner_states, &embedded_states) != Some(false) {
            errors.push(format!(
                "repository-topology row {name:?} is not a root-workspace member, and its owner {:?} still holds embedded source",
                row.current_owner
            ));
        }
    }
    errors
}

/// Whether this package is still built from embedded workspace source.
///
/// The package's own integration mode decides this, not only its owner's state.
/// `extracting` deliberately permits both `workspace_path` and `exact_git_bridge`
/// so a repository can migrate package by package; judging membership from the
/// owner's state alone would demand that a package which has already crossed the
/// bridge still be a workspace member, which is exactly what crossing it undoes.
/// `None` when the owner or its migration state is unknown — both are reported
/// separately, so membership is not judged on a broken reference.
fn expects_embedded_source(
    package: &Package,
    owner_states: &BTreeMap<&str, &str>,
    embedded_states: &BTreeMap<&str, bool>,
) -> Option<bool> {
    let state = owner_states.get(package.current_owner.as_str())?;
    let owner_holds_source = embedded_states.get(state).copied()?;
    // An immutable mode means the consumer takes a published/pinned artifact, so
    // the package is not workspace source regardless of what its owner still holds.
    if IMMUTABLE_MODES.contains(&package.current_integration_mode.as_str()) {
        return Some(false);
    }
    Some(owner_holds_source)
}

/// Read the root workspace members and classify each one from its own manifest.
pub fn read_workspace_manifests(root: &Path) -> Result<Vec<ManifestPackage>> {
    // Cargo, not the literal `members` array, is the authority on membership: a
    // path dependency living under the workspace root becomes a member without
    // ever being listed, and `members` may hold globs. Reading the array alone
    // would under-report exactly the package that arrived unclassified.
    let manifest_path = root.join("Cargo.toml");
    let output = Command::new("cargo")
        .arg("metadata")
        .arg("--format-version")
        .arg("1")
        .arg("--no-deps")
        .arg("--manifest-path")
        .arg(&manifest_path)
        .output()
        .wrap_err("failed to run `cargo metadata`")?;
    if !output.status.success() {
        bail!("cargo metadata failed:\n{}", String::from_utf8_lossy(&output.stderr));
    }
    let metadata: CargoMetadata =
        serde_json::from_slice(&output.stdout).wrap_err("failed to parse cargo metadata")?;
    let members: BTreeSet<&str> = metadata.workspace_members.iter().map(String::as_str).collect();

    let mut packages = Vec::new();
    for package in &metadata.packages {
        if !members.contains(package.id.as_str()) {
            continue;
        }
        let dir = Path::new(&package.manifest_path)
            .parent()
            .ok_or_else(|| color_eyre::eyre::eyre!("{} has no parent", package.manifest_path))?;
        // Fail closed rather than falling back to the absolute manifest path: a
        // wrong-shaped path would surface as a spurious "path does not match"
        // finding on every row and hide the real resolution failure.
        let path = dir
            .strip_prefix(root)
            .map(|rel| rel.to_string_lossy().replace('\\', "/"))
            .wrap_err_with(|| {
                format!(
                    "cargo reported {} outside the repository root {}",
                    dir.display(),
                    root.display()
                )
            })?;
        packages.push(ManifestPackage {
            name: package.name.clone(),
            path,
            // Cargo resolves `publish` to `None` for publishable-anywhere and to
            // an explicit list otherwise; `publish = false` becomes an empty list.
            publish_disposition: match package.publish.as_deref() {
                Some([]) => PublishDisposition::PrivateWorkspaceMember,
                _ => PublishDisposition::Published,
            },
            metadata_authority: read_metadata_authority(dir)?,
        });
    }
    packages.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(packages)
}

/// Whether the package sets `repository` itself or inherits the workspace value.
///
/// This reads the raw manifest on purpose: `cargo metadata` has already resolved
/// `repository.workspace = true` into the inherited string, so the resolved view
/// cannot tell an explicit override from an inherited default.
fn read_metadata_authority(dir: &Path) -> Result<MetadataAuthority> {
    let manifest_path = dir.join("Cargo.toml");
    let text = fs::read_to_string(&manifest_path)
        .wrap_err_with(|| format!("failed to read {}", manifest_path.display()))?;
    let parsed: toml::Value = toml::from_str(&text)
        .wrap_err_with(|| format!("failed to parse {}", manifest_path.display()))?;
    let repository = parsed.get("package").and_then(|package| package.get("repository"));
    Ok(match repository {
        // No key at all: the package advertises no repository and inherits none.
        None => MetadataAuthority::Unset,
        Some(toml::Value::String(_)) => MetadataAuthority::PackageExplicit,
        Some(toml::Value::Table(table))
            if table.get("workspace").and_then(toml::Value::as_bool) == Some(true) =>
        {
            MetadataAuthority::WorkspaceInherited
        }
        Some(other) => bail!(
            "{}: package.repository must be a string or `{{ workspace = true }}`, found {}",
            manifest_path.display(),
            other.type_str()
        ),
    })
}

/// The subset of `cargo metadata --no-deps` this check consumes.
#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    workspace_members: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    id: String,
    name: String,
    manifest_path: String,
    publish: Option<Vec<String>>,
}

/// Directories the root workspace deliberately excludes.
fn read_workspace_excludes(root: &Path) -> Result<BTreeSet<String>> {
    let manifest_path = root.join("Cargo.toml");
    let text = fs::read_to_string(&manifest_path)
        .wrap_err_with(|| format!("failed to read {}", manifest_path.display()))?;
    let parsed: toml::Value = toml::from_str(&text)
        .wrap_err_with(|| format!("failed to parse {}", manifest_path.display()))?;
    Ok(parsed
        .get("workspace")
        .and_then(|workspace| workspace.get("exclude"))
        .and_then(toml::Value::as_array)
        .map(|entries| entries.iter().filter_map(toml::Value::as_str).map(str::to_string).collect())
        .unwrap_or_default())
}

fn placement_label(package: &Package) -> String {
    match package.placement {
        Placement::Accepted => package.future_owner.clone().unwrap_or_else(|| "—".to_string()),
        Placement::Pending => "_pending_".to_string(),
    }
}

/// Escape free-form policy prose for use inside a Markdown table cell.
///
/// Descriptions, open questions and notes are authored prose, so a pipe or a line
/// break in an otherwise valid policy value would silently split or truncate a row.
/// The projection would still compare equal to the checked-in copy — both malformed —
/// so the binding test cannot catch it; escaping at render time is what prevents it.
fn cell(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '|' => escaped.push_str("\\|"),
            '\n' | '\r' => escaped.push(' '),
            _ => escaped.push(character),
        }
    }
    escaped
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
            cell(&state.description)
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
            snake_case_name(&repo.role),
            snake_case_name(&repo.history_transfer),
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
                cell(package.placement_question.as_deref().unwrap_or(""))
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
                tree.path,
                snake_case_name(&tree.kind),
                tree.owner,
                cell(&tree.notes)
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
requires_immutable_consumption = false
accepts_future_packages = true

[[migration_states]]
state = "external"
description = "Owned elsewhere."
legal_integration_modes = ["released_registry"]
allows_embedded_workspace_source = false
requires_immutable_consumption = true
accepts_future_packages = true

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
role = "corpus_assets"
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
    fn excludes() -> BTreeSet<String> {
        BTreeSet::new()
    }

    fn expect_rejected(source: &str, needle: &str) -> TestResult {
        let topology = parse(source)?;
        let error = match validate(&topology, &manifests(), &excludes()) {
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
        let error = match validate(topology, actual, &excludes()) {
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
        validate(&topology, &manifests(), &excludes())?;
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
    fn rejects_a_targetless_repository_in_a_policy_declared_immutable_state() -> TestResult {
        // The rule must follow the state's declared `requires_immutable_consumption`,
        // not a list of state names written in Rust. A state added to the policy file
        // that Rust has never heard of must not escape the requirement by not being
        // named — which is the whole reason that flag is declared rather than inferred.
        let source = format!(
            "{}\n[[migration_states]]\nstate = \"published\"\ndescription = \"Consumed from a registry.\"\nlegal_integration_modes = [\"released_registry\"]\nallows_embedded_workspace_source = false\nrequires_immutable_consumption = true\naccepts_future_packages = false\n",
            fixture()
        )
        .replace(
            "repository_id = \"library\"\ntarget = \"Org/library\"\nmigration_state = \"embedded\"",
            "repository_id = \"library\"\ntarget_decision_owner = \"#9\"\nmigration_state = \"published\"",
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

    /// Without this, deleting the whole path-reconciliation branch leaves the
    /// suite green: real data never disagrees with itself.
    #[test]
    fn rejects_a_row_whose_path_is_not_the_member_path() -> TestResult {
        let topology = parse(&fixture())?;
        let mut actual = manifests();
        actual[0].path = "crates/somewhere-else".into();
        expect_rejected_against(&topology, &actual, "does not match workspace member path")
    }

    /// A package owned by a repository that has taken its source away must still
    /// be representable — otherwise the external states can never hold anything.
    #[test]
    fn accepts_a_package_owned_by_an_external_repository() -> TestResult {
        let source = fixture()
            .replace(
                "repository_id = \"library\"\ntarget = \"Org/library\"\nmigration_state = \"embedded\"",
                "repository_id = \"library\"\ntarget = \"Org/library\"\nmigration_state = \"external\"",
            )
            .replace(
                "name = \"alpha\"\npath = \"crates/alpha\"\ncurrent_owner = \"product\"",
                "name = \"alpha\"\npath = \"crates/alpha\"\ncurrent_owner = \"library\"",
            )
            .replace(
                "migration_owner = \"#3\"\npublish_disposition = \"published\"\ncurrent_integration_mode = \"workspace_path\"",
                "migration_owner = \"#3\"\npublish_disposition = \"published\"\ncurrent_integration_mode = \"released_registry\"",
            );
        let topology = parse(&source)?;
        // `alpha` has left the workspace, so only `beta` remains a member.
        let actual = vec![manifests()[1].clone()];
        validate(&topology, &actual, &excludes())?;
        Ok(())
    }

    /// The other direction: externalized source must not still be in the workspace.
    #[test]
    fn rejects_an_externalized_package_still_in_the_workspace() -> TestResult {
        let source = fixture()
            .replace(
                "repository_id = \"library\"\ntarget = \"Org/library\"\nmigration_state = \"embedded\"",
                "repository_id = \"library\"\ntarget = \"Org/library\"\nmigration_state = \"external\"",
            )
            .replace(
                "name = \"alpha\"\npath = \"crates/alpha\"\ncurrent_owner = \"product\"",
                "name = \"alpha\"\npath = \"crates/alpha\"\ncurrent_owner = \"library\"",
            )
            .replace(
                "migration_owner = \"#3\"\npublish_disposition = \"published\"\ncurrent_integration_mode = \"workspace_path\"",
                "migration_owner = \"#3\"\npublish_disposition = \"published\"\ncurrent_integration_mode = \"released_registry\"",
            );
        // `alpha` is external but still a workspace member.
        expect_rejected(&source, "still a root-workspace member")
    }

    /// `retired` declares no immutable modes; inferring the requirement from
    /// "no embedded source" made it a state no package could ever satisfy.
    #[test]
    fn accepts_a_state_with_no_embedded_source_and_no_immutable_requirement() -> TestResult {
        let source = fixture().replace(
            "state = \"external\"\ndescription = \"Owned elsewhere.\"\nlegal_integration_modes = [\"released_registry\"]\nallows_embedded_workspace_source = false\nrequires_immutable_consumption = true",
            "state = \"external\"\ndescription = \"No longer consumed.\"\nlegal_integration_modes = [\"none\"]\nallows_embedded_workspace_source = false\nrequires_immutable_consumption = false",
        );
        let topology = parse(&source)?;
        validate(&topology, &manifests(), &excludes())?;
        Ok(())
    }

    #[test]
    fn rejects_a_state_requiring_immutability_but_listing_a_mutable_mode() -> TestResult {
        let source = fixture().replace(
            "legal_integration_modes = [\"released_registry\"]\nallows_embedded_workspace_source = false\nrequires_immutable_consumption = true",
            "legal_integration_modes = [\"released_registry\", \"none\"]\nallows_embedded_workspace_source = false\nrequires_immutable_consumption = true",
        );
        expect_rejected(&source, "lists mutable mode")
    }

    /// Without this, the package-level immutable-consumption branch is reachable
    /// but unasserted: disabling it left the whole suite green.
    #[test]
    fn rejects_a_mutable_consumption_mode_under_an_immutable_owner() -> TestResult {
        // `alpha` moves under the external owner but keeps a workspace path.
        let source = fixture().replace(
            "name = \"alpha\"\npath = \"crates/alpha\"\ncurrent_owner = \"product\"",
            "name = \"alpha\"\npath = \"crates/alpha\"\ncurrent_owner = \"library\"",
        );
        let source = source.replace(
            "repository_id = \"library\"\ntarget = \"Org/library\"\nmigration_state = \"embedded\"",
            "repository_id = \"library\"\ntarget = \"Org/library\"\nmigration_state = \"external\"",
        );
        expect_rejected(&source, "requires an immutable consumption mode")
    }

    /// The mirror of the rule below: a state that keeps embedded source must
    /// permit the mode that expresses it, or no package can ever satisfy it.
    #[test]
    fn rejects_a_state_keeping_embedded_source_without_workspace_path() -> TestResult {
        let source = fixture().replace(
            "legal_integration_modes = [\"workspace_path\"]\nallows_embedded_workspace_source = true",
            "legal_integration_modes = [\"exact_git_bridge\"]\nallows_embedded_workspace_source = true",
        );
        expect_rejected(&source, "does not list \"workspace_path\" as legal")
    }

    #[test]
    fn rejects_a_state_forbidding_embedded_source_but_allowing_workspace_path() -> TestResult {
        let source = fixture().replace(
            "legal_integration_modes = [\"released_registry\"]\nallows_embedded_workspace_source = false",
            "legal_integration_modes = [\"released_registry\", \"workspace_path\"]\nallows_embedded_workspace_source = false",
        );
        expect_rejected(&source, "forbids embedded source but lists")
    }

    #[test]
    fn rejects_issue_zero_and_leading_zero_refs() {
        assert!(!is_issue_ref("#0"), "#0 cannot resolve to a real issue");
        assert!(!is_issue_ref("#007675"), "a leading zero gives one issue two spellings");
        assert!(!is_issue_ref("#"), "a bare hash is not a reference");
        assert!(!is_issue_ref("7675"), "a reference must carry its hash");
        assert!(is_issue_ref("#7675"), "an ordinary reference must still pass");
    }

    #[test]
    fn rejects_duplicate_dependency_entries() -> TestResult {
        let source = fixture().replace(
            "allowed_dependencies = [\"library\"]",
            "allowed_dependencies = [\"library\", \"library\"]",
        );
        expect_rejected(&source, "allowed_dependencies contains a duplicate")
    }

    #[test]
    fn rejects_an_excluded_tree_the_workspace_does_not_exclude() -> TestResult {
        let source = format!(
            "{}\n[[excluded_trees]]\npath = \"vendor\"\nkind = \"vendored_upstream_source\"\nowner = \"#9\"\nnotes = \"n\"\n",
            fixture()
        );
        expect_rejected(&source, "this row is stale")
    }

    #[test]
    fn accepts_an_excluded_tree_the_workspace_really_excludes() -> TestResult {
        let source = format!(
            "{}\n[[excluded_trees]]\npath = \"vendor\"\nkind = \"vendored_upstream_source\"\nowner = \"#9\"\nnotes = \"n\"\n",
            fixture()
        );
        let topology = parse(&source)?;
        let excludes = BTreeSet::from(["vendor".to_string()]);
        validate(&topology, &manifests(), &excludes)?;
        Ok(())
    }

    /// `role`, `history_transfer` and `kind` are closed vocabularies. A typo in
    /// any of them used to pass validation untouched and land in the generated
    /// page, because nothing but the renderer ever read them.
    #[test]
    fn rejects_an_unrecognised_repository_role() {
        let source = fixture().replace("role = \"corpus_assets\"", "role = \"corpus_asets\"");
        assert!(parse(&source).is_err(), "an unrecognised role must be rejected");
    }

    #[test]
    fn rejects_an_unrecognised_history_transfer() {
        let source = fixture()
            .replace("history_transfer = \"not_applicable\"", "history_transfer = \"maybe\"");
        assert!(parse(&source).is_err(), "an unrecognised history transfer must be rejected");
    }

    #[test]
    fn rejects_an_unrecognised_excluded_tree_kind() {
        let source = format!(
            "{}\n[[excluded_trees]]\npath = \"vendor\"\nkind = \"mystery\"\nowner = \"#9\"\nnotes = \"n\"\n",
            fixture()
        );
        assert!(parse(&source).is_err(), "an unrecognised excluded-tree kind must be rejected");
    }

    /// A repository nothing consumes any more cannot be where a package is headed.
    #[test]
    fn rejects_a_future_owner_that_accepts_no_packages() -> TestResult {
        // `library` is `alpha`'s accepted destination and sits in the first
        // declared state, so flipping that state's flag makes it a non-destination.
        let source = fixture().replacen(
            "accepts_future_packages = true",
            "accepts_future_packages = false",
            1,
        );
        expect_rejected(&source, "which accepts no packages")
    }

    /// Identity must hold for an externalized package too: its row is never
    /// reconciled against a manifest, so nothing else would catch a blank.
    #[test]
    fn rejects_a_blank_package_name() -> TestResult {
        let source = fixture().replace("name = \"beta\"", "name = \"\"");
        expect_rejected(&source, "name is empty")
    }

    #[test]
    fn rejects_a_blank_package_path() -> TestResult {
        let source = fixture().replace("path = \"crates/beta\"", "path = \"\"");
        expect_rejected(&source, "path is empty")
    }

    #[test]
    fn rejects_a_package_path_that_escapes_the_repository() -> TestResult {
        let source = fixture().replace("path = \"crates/beta\"", "path = \"../outside\"");
        expect_rejected(&source, "repository-relative path inside the tree")
    }

    #[test]
    fn rejects_an_absolute_package_path() -> TestResult {
        let source = fixture().replace("path = \"crates/beta\"", "path = \"/etc/beta\"");
        expect_rejected(&source, "repository-relative path inside the tree")
    }

    /// `Path::components` folds these away, so each one would otherwise reach the
    /// projection as a spelling that never equals the manifest path.
    #[test]
    fn rejects_noncanonical_package_paths() -> TestResult {
        for spelling in ["./crates/beta", "crates/beta/", "crates//beta", "crates/./beta"] {
            let source =
                fixture().replace("path = \"crates/beta\"", &format!("path = {spelling:?}"));
            expect_rejected(&source, "is not canonical")?;
        }
        Ok(())
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
        let excludes = read_workspace_excludes(&root)?;
        validate(&topology, &manifests, &excludes)?;
        Ok(())
    }

    /// Cargo, not the literal `members` array, decides membership. If this ever
    /// disagrees, an implicit member could sit unclassified while the gate stays
    /// green.
    #[test]
    fn workspace_membership_comes_from_cargo() -> TestResult {
        let root = project_root()?;
        let resolved = read_workspace_manifests(&root)?;
        let text = fs::read_to_string(root.join("Cargo.toml"))?;
        let parsed: toml::Value = toml::from_str(&text)?;
        let literal = parsed
            .get("workspace")
            .and_then(|workspace| workspace.get("members"))
            .and_then(toml::Value::as_array)
            .map(Vec::len)
            .unwrap_or_default();
        if resolved.len() < literal {
            bail!("cargo resolved {} members, fewer than the {literal} listed", resolved.len());
        }
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
    fn accepts_a_bridged_package_under_an_owner_that_still_holds_source() -> TestResult {
        // `extracting` permits both `workspace_path` and `exact_git_bridge` so a
        // repository can migrate package by package. Judging membership from the
        // owner's state alone would demand that a package which has already crossed
        // the bridge still be a workspace member — the state would be a trap, the
        // same shape as the earlier unreachable `external` and `retired` states.
        let source = fixture()
            .replacen(
                "state = \"embedded\"\ndescription = \"Source lives here.\"\nlegal_integration_modes = [\"workspace_path\"]",
                "state = \"embedded\"\ndescription = \"Source lives here.\"\nlegal_integration_modes = [\"workspace_path\", \"exact_git_bridge\"]",
                1,
            )
;
        // `beta` crosses the bridge while its owner still holds `alpha` as source.
        let source = swap_beta_to_bridge(&source);
        let topology = parse(&source)?;
        let only_alpha = vec![manifests()[0].clone()];
        validate(&topology, &only_alpha, &excludes())?;
        Ok(())
    }

    /// Point `beta`'s integration mode at the immutable bridge, leaving `alpha` alone.
    fn swap_beta_to_bridge(source: &str) -> String {
        let Some(beta_at) = source.find("name = \"beta\"") else {
            return source.to_owned();
        };
        let (head, tail) = source.split_at(beta_at);
        format!(
            "{head}{}",
            tail.replacen(
                "current_integration_mode = \"workspace_path\"",
                "current_integration_mode = \"exact_git_bridge\"",
                1,
            )
        )
    }

    #[test]
    fn rejects_an_excluded_tree_path_that_escapes_the_repository() -> TestResult {
        // Held to the same shape rule as a package path: an escaping path would have
        // this authority claim files outside the repository. The workspace exclude
        // list is made to contain the same string, so staleness cannot be what
        // rejects it — only the shape check can.
        let source = format!(
            "{}\n[[excluded_trees]]\npath = \"../vendor\"\nkind = \"vendored_upstream_source\"\nowner = \"#9\"\nnotes = \"n\"\n",
            fixture()
        );
        let topology = parse(&source)?;
        let excludes = BTreeSet::from(["../vendor".to_string()]);
        let Err(error) = validate(&topology, &manifests(), &excludes) else {
            bail!("an escaping excluded-tree path was accepted");
        };
        let message = format!("{error}");
        if !message.contains("must be a repository-relative path inside the tree") {
            bail!("unexpected rejection reason: {message}");
        }
        Ok(())
    }

    #[test]
    fn rejects_a_blank_excluded_tree_note() -> TestResult {
        let source = format!(
            "{}\n[[excluded_trees]]\npath = \"vendor\"\nkind = \"vendored_upstream_source\"\nowner = \"#9\"\nnotes = \"  \"\n",
            fixture()
        );
        let topology = parse(&source)?;
        let excludes = BTreeSet::from(["vendor".to_string()]);
        let Err(error) = validate(&topology, &manifests(), &excludes) else {
            bail!("a blank excluded-tree note was accepted");
        };
        let message = format!("{error}");
        if !message.contains("notes is empty") {
            bail!("unexpected rejection reason: {message}");
        }
        Ok(())
    }

    #[test]
    fn rejects_a_blank_migration_state_name() -> TestResult {
        let source = fixture().replacen("state = \"embedded\"", "state = \"\"", 1);
        expect_rejected(&source, "state is empty")
    }

    #[test]
    fn rejects_a_blank_migration_state_description() -> TestResult {
        let source =
            fixture().replacen("description = \"Source lives here.\"", "description = \"  \"", 1);
        expect_rejected(&source, "description is empty")
    }

    #[test]
    fn rejects_a_blank_integration_mode_entry() -> TestResult {
        let source = fixture().replacen(
            "legal_integration_modes = [\"workspace_path\"]",
            "legal_integration_modes = [\"workspace_path\", \"\"]",
            1,
        );
        expect_rejected(&source, "legal_integration_modes entry is empty")
    }

    #[test]
    fn rejects_two_package_rows_claiming_one_path() -> TestResult {
        // Reconciliation cannot catch this once the owner has externalized, because
        // neither row is compared against a manifest then.
        let source = fixture().replace("path = \"crates/beta\"", "path = \"crates/alpha\"");
        expect_rejected(&source, "is already claimed by another package row")
    }

    fn manifest_dir(body: &str) -> Result<tempfile::TempDir> {
        let dir = tempfile::tempdir().wrap_err("failed to create temp dir")?;
        fs::write(dir.path().join("Cargo.toml"), body).wrap_err("failed to write manifest")?;
        Ok(dir)
    }

    #[test]
    fn a_missing_repository_key_is_not_reported_as_inherited() -> TestResult {
        // A package that omits `repository` inherits nothing. Recording it as
        // `workspace_inherited` would assert a repository Cargo never resolved for it.
        let dir = manifest_dir("[package]\nname = \"alpha\"\nversion = \"0.1.0\"\n")?;
        assert_eq!(read_metadata_authority(dir.path())?, MetadataAuthority::Unset);
        Ok(())
    }

    #[test]
    fn an_inherited_repository_is_distinguished_from_an_explicit_one() -> TestResult {
        let inherited = manifest_dir(
            "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\nrepository.workspace = true\n",
        )?;
        assert_eq!(
            read_metadata_authority(inherited.path())?,
            MetadataAuthority::WorkspaceInherited
        );

        let explicit = manifest_dir(
            "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\nrepository = \"https://example.invalid/a\"\n",
        )?;
        assert_eq!(read_metadata_authority(explicit.path())?, MetadataAuthority::PackageExplicit);
        Ok(())
    }

    #[test]
    fn rejects_a_malformed_repository_field() -> TestResult {
        // `repository.workspace = false` inherits nothing and overrides nothing, so
        // silently classifying it either way would record a fact that is not true.
        let dir = manifest_dir(
            "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\nrepository = { workspace = false }\n",
        )?;
        let Err(error) = read_metadata_authority(dir.path()) else {
            bail!("a malformed package.repository was accepted");
        };
        let message = format!("{error}");
        if !message.contains("must be a string or") {
            bail!("unexpected rejection reason: {message}");
        }
        Ok(())
    }

    #[test]
    fn projection_escapes_prose_that_would_break_a_table_row() -> TestResult {
        // A pipe or a newline in authored policy prose must not split or truncate the
        // row. The binding test cannot catch this: a malformed projection still equals
        // its malformed checked-in copy, so escaping has to happen at render time.
        let source = fixture().replace("Source lives here.", "Pipe | inside\\nand a newline.");
        let rendered = render(&parse(&source)?);
        let Some(row) = rendered.lines().find(|line| line.contains("Pipe")) else {
            bail!("no rendered row carried the prose");
        };
        assert!(row.contains("Pipe \\| inside and a newline."), "unescaped row: {row}");
        // Four columns means five delimiters; an unescaped pipe would make six.
        assert_eq!(row.matches(" | ").count(), 3, "row shape changed: {row}");
        Ok(())
    }

    #[test]
    fn escaped_cell_leaves_ordinary_prose_untouched() -> TestResult {
        // Negative control for the escaper itself: it must not rewrite normal text.
        assert_eq!(
            cell("Still embedded; extraction not started."),
            "Still embedded; extraction not started."
        );
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
