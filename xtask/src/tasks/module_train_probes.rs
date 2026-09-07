//! Semantic current-tree implementation probes for the module train (#11626).
//!
//! This registry lives in its own module on purpose. A selector matches plain
//! text in a tree file, so if the anchor string literals sat in the same file a
//! selector targets, that selector could be satisfied by *its own declaration*
//! rather than by real code — a vacuous probe that would survive deleting the
//! implementation it claims to verify.
//!
//! Keeping `PROBED_NODES` here, and never targeting this file from a selector,
//! makes that impossible by construction. The guard test
//! `no_selector_targets_the_registry_that_declares_it` pins the rule.

use color_eyre::eyre::{Context, Result};
use std::path::PathBuf;

use super::{ProbeOutcome, TrainNode};

// ---------------------------------------------------------------------------
// Semantic current-tree implementation probes (#11626 residual 1).
//
// A probe must not be satisfiable by source-file presence alone: that is this
// issue's falsifier 2 ("source/type/generated file exists while the production
// consumer still uses a legacy route"). Every component therefore asserts both
// the implementing surface AND the production consumer that dispatches to it,
// so an orphaned module or an unwired command cannot read as landed.
//
// Selectors are read through a `TreeSource` rather than the compiler so the
// negative direction stays testable: a tree missing the consumer must be able
// to fail the probe.
// ---------------------------------------------------------------------------

/// Read-only access to the inspected tree's text files.
pub trait TreeSource {
    /// Read a repository-relative text file. `Ok(None)` means the path is
    /// absent; an `Err` is an instrument failure and never a silent absence.
    fn read_text(&self, relative: &str) -> Result<Option<String>>;
}

/// The real tree source: files under the repository root the manifest was
/// loaded from, never the ambient process working directory.
pub struct RepoTreeSource {
    root: PathBuf,
}

impl RepoTreeSource {
    pub fn from_project_root() -> Result<Self> {
        Ok(Self { root: crate::utils::project_root()? })
    }
}

impl TreeSource for RepoTreeSource {
    fn read_text(&self, relative: &str) -> Result<Option<String>> {
        let path = self.root.join(relative);
        match std::fs::read(&path) {
            Ok(bytes) => {
                let text = String::from_utf8(bytes).with_context(|| {
                    format!("probe selector target {relative} is not valid UTF-8")
                })?;
                Ok(Some(text))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => {
                Err(err).with_context(|| format!("failed to read probe selector target {relative}"))
            }
        }
    }
}

/// One file that must carry every listed anchor for its component to be met.
struct ComponentSelector {
    path: &'static str,
    anchors: &'static [&'static str],
}

/// One declared component of a node's positive surface.
struct ComponentProbe {
    component: &'static str,
    selectors: &'static [ComponentSelector],
}

/// The semantic probe contract for one train node.
struct NodeProbeContract {
    node_id: &'static str,
    issue: u64,
    components: &'static [ComponentProbe],
}

/// Nodes carrying a semantic current-tree probe.
///
/// C01 is not listed: its declared implementation IS the validated
/// `module_train.v1` data bundle, so the loaded manifest at the pinned digest
/// is its positive surface and it is decided before this registry.
///
/// Every other train node remains deliberately unlisted and therefore
/// `not_proven` — a probe is written when its node's surface is established,
/// never inferred from a merge, an issue state, or a filename.
const PROBED_NODES: &[NodeProbeContract] = &[
    NodeProbeContract {
        node_id: "C02",
        issue: 11626,
        components: &[
            ComponentProbe {
                component: "current_tree_probes",
                selectors: &[
                    ComponentSelector {
                        path: "xtask/src/tasks/module_train.rs",
                        anchors: &["fn project_states", "probes::node_probe"],
                    },
                    ComponentSelector {
                        path: "xtask/src/main.rs",
                        anchors: &["ModuleTrainCommand::Status", "module_train::run_status"],
                    },
                ],
            },
            ComponentProbe {
                component: "offline_frontier",
                selectors: &[
                    ComponentSelector {
                        path: "xtask/src/tasks/module_train.rs",
                        anchors: &["fn render_next"],
                    },
                    ComponentSelector {
                        path: "xtask/src/main.rs",
                        anchors: &["ModuleTrainCommand::Next", "module_train::run_next"],
                    },
                ],
            },
            // Recorded residual of #11626: the offline static agent packet and
            // graph projection. The live `explain` under C03 composes a live
            // addendum and is a different subject; it must not satisfy this
            // component, so the anchors name the offline command explicitly.
            ComponentProbe {
                component: "static_packet_projection",
                selectors: &[ComponentSelector {
                    path: "xtask/src/main.rs",
                    anchors: &["ModuleTrainCommand::Explain", "module_train::run_explain"],
                }],
            },
        ],
    },
    NodeProbeContract {
        node_id: "C03",
        issue: 11627,
        components: &[
            ComponentProbe {
                component: "live_snapshot_normalization",
                selectors: &[
                    ComponentSelector {
                        path: "xtask/src/tasks/module_train_live.rs",
                        anchors: &[
                            "pub struct LiveSnapshot",
                            "pub fn run_refresh",
                            "pub fn run_check",
                        ],
                    },
                    ComponentSelector {
                        path: "xtask/src/main.rs",
                        anchors: &[
                            "ModuleTrainLiveCommand::Refresh",
                            "module_train_live::run_refresh",
                            "ModuleTrainLiveCommand::Check",
                            "module_train_live::run_check",
                        ],
                    },
                ],
            },
            ComponentProbe {
                component: "action_classification",
                selectors: &[
                    ComponentSelector {
                        path: "xtask/src/tasks/module_train_live.rs",
                        anchors: &["pub enum Action", "pub fn classify"],
                    },
                    ComponentSelector {
                        path: "xtask/src/main.rs",
                        anchors: &[
                            "ModuleTrainLiveCommand::Next",
                            "module_train_live::run_next",
                            "ModuleTrainLiveCommand::Explain",
                            "module_train_live::run_explain",
                        ],
                    },
                ],
            },
        ],
    },
];

/// Probe one node's implementation presence against the inspected tree.
///
/// Returns the outcome plus the sorted names of any declared components that
/// are not present, so a partial implementation reports exactly what is
/// missing instead of collapsing into a bare `not_proven`.
pub(super) fn node_probe(
    node: &TrainNode,
    tree: &dyn TreeSource,
) -> Result<(ProbeOutcome, Vec<String>)> {
    // C01's declared implementation is the validated manifest bundle itself,
    // which the loader already verified at the pinned canonical digest.
    if node.node_id == "C01" && node.issue == 11625 {
        return Ok((ProbeOutcome::Pass, Vec::new()));
    }

    let Some(contract) = PROBED_NODES
        .iter()
        .find(|contract| contract.node_id == node.node_id && contract.issue == node.issue)
    else {
        return Ok((ProbeOutcome::Unprobed, Vec::new()));
    };

    let mut met = 0usize;
    let mut unmet: Vec<String> = Vec::new();
    for component in contract.components {
        let mut satisfied = true;
        for selector in component.selectors {
            let Some(text) = tree.read_text(selector.path)? else {
                satisfied = false;
                break;
            };
            if !selector.anchors.iter().all(|anchor| text.contains(anchor)) {
                satisfied = false;
                break;
            }
        }
        if satisfied {
            met += 1;
        } else {
            unmet.push(component.component.to_string());
        }
    }

    unmet.sort();
    let outcome = if unmet.is_empty() {
        ProbeOutcome::Pass
    } else if met == 0 {
        ProbeOutcome::NotPresent
    } else {
        ProbeOutcome::Partial
    };
    Ok((outcome, unmet))
}

/// Repository-relative path of this file — the module that declares
/// `PROBED_NODES`. No selector may target it: matching plain text in the file
/// that holds the anchor literals would let a component be satisfied by its own
/// declaration instead of by real code.
#[cfg(test)]
pub(super) const REGISTRY_RELATIVE_PATH: &str = "xtask/src/tasks/module_train_probes.rs";

/// Every distinct file path any selector targets, for the anti-vacuity guard.
#[cfg(test)]
pub(super) fn selector_paths() -> Vec<&'static str> {
    let mut paths: Vec<&'static str> = PROBED_NODES
        .iter()
        .flat_map(|contract| contract.components)
        .flat_map(|component| component.selectors)
        .map(|selector| selector.path)
        .collect();
    paths.sort_unstable();
    paths.dedup();
    paths
}

/// Every `(path, anchor)` pair any selector asserts, for the anti-vacuity guard.
#[cfg(test)]
pub(super) fn selector_anchors() -> Vec<(&'static str, &'static str)> {
    let mut pairs: Vec<(&'static str, &'static str)> = PROBED_NODES
        .iter()
        .flat_map(|contract| contract.components)
        .flat_map(|component| component.selectors)
        .flat_map(|selector| selector.anchors.iter().map(move |anchor| (selector.path, *anchor)))
        .collect();
    pairs.sort_unstable();
    pairs.dedup();
    pairs
}
