//! Fail-closed affected routing and omitted-new-target discovery (#12411).
//!
//! Selection is prefix-based over registered row subjects. Changes inside the
//! control plane itself (the register files or this module) select every
//! active cohort row: a control-plane change is exactly the affected surface.
//! Unrelated changes produce a checked scoped no-op that names each changed
//! file and why no row selected it; they never force the full denominator.
//! A change intersecting only dormant rows fails loudly instead of silently
//! skipping.

use crate::test_topology::model::{RouteClass, TargetStatus, TopologyRegister};
use crate::test_topology::receipts::{ClassifiedFile, ScopedNoopProof};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Prefixes selecting all active rows of a cohort (control-plane subjects).
pub const CONTROL_PLANE_PREFIXES: &[&str] = &[".ci/test-topology/", "xtask/src/test_topology"];

/// Normalize a repo-relative path to forward slashes.
fn normalize(path: &str) -> String {
    path.replace('\\', "/")
}

/// Outcome of running the canonical selector over one changed-file list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionDecision {
    /// Active required rows whose subjects intersected the changes.
    Selected {
        /// Target ids to route (required class only).
        target_ids: Vec<String>,
        /// True when the control-plane itself changed (select-all surface).
        control_plane_change: bool,
    },
    /// No row selected; a checked scoped no-op proof names every file.
    ScopedNoop(ScopedNoopProof),
    /// Nothing changed at all; an empty scope is explicit rather than silent.
    EmptyChangeSet,
}

impl SelectionDecision {
    /// Selected required target ids; empty for no-op and empty-change shapes.
    pub fn selected_target_ids(&self) -> &[String] {
        match self {
            Self::Selected { target_ids, .. } => target_ids,
            Self::ScopedNoop(_) | Self::EmptyChangeSet => &[],
        }
    }
}

/// Compute the selected scope for one candidate over the canonical register.
///
/// Dormant rows never enter `target_ids`; when their subjects intersect the
/// changes, [`SelectionResult`] records them so callers can fail loudly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionResult {
    /// The routing decision.
    pub decision: SelectionDecision,
    /// Dormant rows hit by the changes (routing must refuse these scopes).
    pub dormant_selected: Vec<String>,
}

/// Run the canonical selector.
///
/// `changed` holds repo-relative paths from the diff (unsorted is fine);
/// cohort identity comes from the register. A single unclassifiable change
/// still produces its classification reason in any scoped no-op.
pub fn select_active_scope(register: &TopologyRegister, changed: &[PathBuf]) -> SelectionResult {
    let changed_norm: Vec<String> =
        changed.iter().map(|path| normalize(&path.to_string_lossy())).collect();
    if changed.is_empty() {
        return SelectionResult {
            decision: SelectionDecision::EmptyChangeSet,
            dormant_selected: Vec::new(),
        };
    }

    let control_plane_change = changed_norm
        .iter()
        .any(|path| CONTROL_PLANE_PREFIXES.iter().any(|prefix| path.starts_with(prefix)));

    let mut selected = BTreeSet::new();
    let mut dormant_selected = BTreeSet::new();
    if !control_plane_change {
        for row in register.rows() {
            if row.subjects.iter().any(|subject| {
                let subject = normalize(subject);
                changed_norm
                    .iter()
                    .any(|file| file.starts_with(&subject) || subject.starts_with(file.as_str()))
            }) {
                match row.status {
                    TargetStatus::Active => match row.route_class {
                        RouteClass::RequiredAffected => {
                            selected.insert(row.target_id.clone());
                        }
                        // Advisory/scheduled/manual rows are opt-in lanes;
                        // affected PR routing never auto-selects them.
                        RouteClass::Advisory | RouteClass::Scheduled | RouteClass::Manual => {}
                    },
                    TargetStatus::DeclaredPending => {
                        dormant_selected.insert(row.target_id.clone());
                    }
                }
            }
        }
    } else {
        for row in register.rows() {
            if matches!(row.status, TargetStatus::Active)
                && matches!(row.route_class, RouteClass::RequiredAffected)
            {
                selected.insert(row.target_id.clone());
            }
        }
    }

    let decision = if selected.is_empty() && dormant_selected.is_empty() {
        SelectionDecision::ScopedNoop(ScopedNoopProof {
            cohort: register.cohort.clone(),
            classified_files: changed_norm
                .iter()
                .map(|file| ClassifiedFile {
                    path: file.clone(),
                    reason:
                        "outside registered compiler-profile subjects after canonical selection"
                            .to_owned(),
                })
                .collect(),
            head_sha: String::new(),
        })
    } else {
        SelectionDecision::Selected {
            target_ids: selected.into_iter().collect(),
            control_plane_change,
        }
    };

    SelectionResult { decision, dormant_selected: dormant_selected.into_iter().collect() }
}

/// Workspace test-target discovery result for the omitted-new-target guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredTestTarget {
    /// Package name owning the discovered test target.
    pub package: String,
    /// Test-target name as declared by Cargo metadata.
    pub target_name: String,
}

/// Omitted-new-target violations found by the drift guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryViolation {
    /// A workspace test target matches a namespace marker but no register row
    /// answers for it — a new leaf omitted from the checked topology.
    OmittedNewTarget { package: String, target_name: String },
}

/// Enforce register membership over discovered workspace test targets.
///
/// Only targets inside `register.watch_packages` packages are scanned; only
/// targets whose names contain one of `register.namespace_markers` require a
/// registered answer (`execution.cargo_target == name`). Returns all misses.
pub fn check_discovery_membership(
    register: &TopologyRegister,
    discovered: &[DiscoveredTestTarget],
) -> Vec<DiscoveryViolation> {
    let watched: BTreeSet<&str> = register.watch_packages.iter().map(String::as_str).collect();
    let mut known_targets: BTreeSet<(&str, &str)> = BTreeSet::new();
    for row in register.rows() {
        if let Some(execution) = &row.execution {
            if let Some(target_name) = execution.cargo_test_target_name() {
                known_targets.insert((execution.cargo_package(), target_name));
            }
        }
    }
    discovered
        .iter()
        .filter(|target| {
            watched.contains(target.package.as_str())
                && register
                    .namespace_markers
                    .iter()
                    .any(|marker| target.target_name.contains(marker.as_str()))
        })
        .filter(|target| {
            !known_targets.contains(&(target.package.as_str(), target.target_name.as_str()))
        })
        .map(|target| DiscoveryViolation::OmittedNewTarget {
            package: target.package.clone(),
            target_name: target.target_name.clone(),
        })
        .collect()
}

impl crate::test_topology::model::ExecutionKind {
    /// Integration-test binary name for membership checks.
    pub fn cargo_test_target_name(&self) -> Option<&str> {
        match self {
            Self::CargoTest { test_target, .. } => test_target.as_deref(),
        }
    }

    /// Render the exact command argv used for execution and receipts.
    ///
    /// Feature/build flags from the row are inserted verbatim so a receipt's
    /// command text names the exact profile the run proved.
    pub fn render_argv(&self) -> Vec<String> {
        match self {
            Self::CargoTest { package, test_target, filter, feature_profile } => {
                let mut argv =
                    vec!["cargo".to_owned(), "test".to_owned(), "-p".to_owned(), package.clone()];
                if let Some(target) = test_target {
                    argv.push("--test".to_owned());
                    argv.push(target.clone());
                } else {
                    argv.push("--lib".to_owned());
                }
                argv.push("--locked".to_owned());
                for flag in feature_profile.split_whitespace() {
                    if flag != "--locked" {
                        argv.push(flag.to_owned());
                    }
                }
                if !filter.is_empty() {
                    argv.push("--".to_owned());
                    argv.push(filter.clone());
                }
                argv
            }
        }
    }
}

/// Classify whether `path` lies under `root` (both repo-relative).
pub fn path_under_root(root: &Path, path: &Path) -> bool {
    path.starts_with(root)
}

/// Discovered workspace package/test-target metadata row from cargo metadata.
#[derive(Debug, serde::Deserialize)]
struct MetadataPackage {
    name: String,
    targets: Vec<MetadataTarget>,
}

#[derive(Debug, serde::Deserialize)]
struct MetadataTarget {
    name: String,
    kind: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
struct MetadataDocument {
    packages: Vec<MetadataPackage>,
}

/// Discover every workspace test target through cargo metadata (no builds).
///
/// This is the deterministic input of the omitted-new-target drift guard: a
/// new leaf whose test target matches a cohort namespace marker but has no
/// registered topology row fails the check instead of entering CI silently.
pub fn discover_workspace_test_targets(root: &Path) -> anyhow::Result<Vec<DiscoveredTestTarget>> {
    let output = duct::cmd("cargo", ["metadata", "--format-version", "1", "--no-deps"])
        .dir(root)
        .stderr_capture()
        .read()
        .map_err(|error| anyhow::anyhow!("cargo metadata failed: {error}"))?;
    let document: MetadataDocument = serde_json::from_str(&output)
        .map_err(|error| anyhow::anyhow!("parse metadata: {error}"))?;
    let mut discovered = Vec::new();
    for package in document.packages {
        for target in package.targets {
            if target.kind.iter().any(|kind| kind == "test") {
                discovered.push(DiscoveredTestTarget {
                    package: package.name.clone(),
                    target_name: target.name,
                });
            }
        }
    }
    Ok(discovered)
}
