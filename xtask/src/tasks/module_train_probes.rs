//! Semantic current-tree implementation probes for the module train (#11626).
//!
//! This registry lives in its own module on purpose: selectors must never
//! target the declarations that define them. Syntax matching separately
//! rejects comments and string literals, while registry separation keeps
//! self-targeting out of the selector topology entirely.
//!
//! Keeping `PROBED_NODES` here, and never targeting this file from a selector,
//! makes that impossible by construction. The guard test
//! `no_selector_targets_the_registry_that_declares_it` pins the rule.

use crate::tasks::staged::{StagedPathText, read_staged_path_text};
use color_eyre::eyre::{Context, Result, bail};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use syn::visit::Visit;

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
    revision: Option<String>,
    cache: RefCell<BTreeMap<String, Option<String>>>,
}

impl RepoTreeSource {
    pub fn from_project_root() -> Result<Self> {
        Self::from_root(crate::utils::project_root()?, None)
    }

    pub(crate) fn from_root_at_revision(root: PathBuf, revision: String) -> Result<Self> {
        Self::from_root(root, Some(revision))
    }

    fn from_root(root: PathBuf, revision: Option<String>) -> Result<Self> {
        Ok(Self { root, revision, cache: RefCell::new(BTreeMap::new()) })
    }
}

impl TreeSource for RepoTreeSource {
    fn read_text(&self, relative: &str) -> Result<Option<String>> {
        if let Some(cached) = self.cache.borrow().get(relative) {
            return Ok(cached.clone());
        }
        let text = if let Some(revision) = &self.revision {
            match read_staged_path_text(&self.root, relative, Some(revision))? {
                StagedPathText::Present(text) => Some(text),
                StagedPathText::Absent => None,
                StagedPathText::Binary => {
                    bail!("probe selector target {relative} in {revision} is not valid UTF-8")
                }
            }
        } else {
            let path = self.root.join(relative);
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let text = String::from_utf8(bytes).with_context(|| {
                        format!("probe selector target {relative} is not valid UTF-8")
                    })?;
                    Some(text)
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
                Err(err) => {
                    return Err(err).with_context(|| {
                        format!("failed to read probe selector target {relative}")
                    });
                }
            }
        };
        self.cache.borrow_mut().insert(relative.to_string(), text.clone());
        Ok(text)
    }
}

/// One file that must carry every listed syntax anchor for its component to be met.
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

/// Match the finite Rust syntax forms used by the module-train registry.
///
/// Parsing before matching keeps comments and string literals from satisfying
/// a production implementation probe. This intentionally handles only the
/// declaration and dispatch forms registered below; it is not a general Rust
/// source index.
fn semantic_anchor_present(source: &str, anchor: &str) -> bool {
    let Ok(file) = syn::parse_file(source) else {
        return false;
    };

    if let Some(name) = anchor.strip_prefix("pub fn ") {
        return function_present(&file, name, true);
    }
    if let Some(name) = anchor.strip_prefix("fn ") {
        return function_present(&file, name, false);
    }
    if let Some(name) = anchor.strip_prefix("pub struct ") {
        return type_present(&file, name, TypeKind::Struct);
    }
    if let Some(name) = anchor.strip_prefix("pub enum ") {
        return type_present(&file, name, TypeKind::Enum);
    }

    if anchor.starts_with("ModuleTrainCommand::") || anchor.starts_with("ModuleTrainLiveCommand::")
    {
        return dispatch_variant_present(&file, anchor);
    }
    if anchor.starts_with("module_train::") || anchor.starts_with("module_train_live::") {
        return dispatch_call_present(&file, anchor);
    }

    let segments: Vec<&str> = anchor.split("::").collect();
    let mut visitor = PathVisitor { expected: segments, found: false };
    visitor.visit_file(&file);
    visitor.found
}

fn dispatch_variant_present(file: &syn::File, anchor: &str) -> bool {
    let mut visitor = DispatchVisitor {
        expected: anchor.split("::").collect(),
        mode: DispatchMode::Variant,
        in_match_arm: false,
        in_module_train_dispatch: false,
        found: false,
    };
    visitor.visit_file(file);
    visitor.found
}

fn dispatch_call_present(file: &syn::File, anchor: &str) -> bool {
    let mut visitor = DispatchVisitor {
        expected: anchor.split("::").collect(),
        mode: DispatchMode::Call,
        in_match_arm: false,
        in_module_train_dispatch: false,
        found: false,
    };
    visitor.visit_file(file);
    visitor.found
}

#[derive(Clone, Copy)]
enum DispatchMode {
    Variant,
    Call,
}

struct DispatchVisitor<'a> {
    expected: Vec<&'a str>,
    mode: DispatchMode,
    in_match_arm: bool,
    in_module_train_dispatch: bool,
    found: bool,
}

impl<'ast> Visit<'ast> for DispatchVisitor<'_> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if has_test_cfg(&node.attrs) || node.sig.ident != "run_cli" {
            return;
        }
        syn::visit::visit_item_fn(self, node);
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if has_test_cfg(&node.attrs) {
            return;
        }
        syn::visit::visit_item_mod(self, node);
    }

    fn visit_expr_match(&mut self, node: &'ast syn::ExprMatch) {
        self.visit_expr(&node.expr);
        for arm in &node.arms {
            let in_module_train_dispatch =
                pattern_contains_path(&arm.pat, &["Commands", "ModuleTrain"]);
            if matches!(self.mode, DispatchMode::Variant)
                && self.in_module_train_dispatch
                && pattern_contains_path(&arm.pat, &self.expected)
            {
                self.found = true;
            }
            let was_in_match_arm = self.in_match_arm;
            let was_in_module_train_dispatch = self.in_module_train_dispatch;
            self.in_match_arm = true;
            self.in_module_train_dispatch |= in_module_train_dispatch;
            self.visit_arm(arm);
            self.in_match_arm = was_in_match_arm;
            self.in_module_train_dispatch = was_in_module_train_dispatch;
        }
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if matches!(self.mode, DispatchMode::Call)
            && self.in_match_arm
            && self.in_module_train_dispatch
            && expression_path_matches(&node.func, &self.expected)
        {
            self.found = true;
        }
        syn::visit::visit_expr_call(self, node);
    }
}

fn has_test_cfg(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && matches!(&attr.meta, syn::Meta::List(meta) if meta.tokens.to_string().contains("test"))
    })
}

fn pattern_contains_path(pattern: &syn::Pat, expected: &[&str]) -> bool {
    let mut visitor = PathVisitor { expected: expected.to_vec(), found: false };
    visitor.visit_pat(pattern);
    visitor.found
}

fn expression_path_matches(expression: &syn::Expr, expected: &[&str]) -> bool {
    let syn::Expr::Path(path) = expression else {
        return false;
    };
    path.path.segments.len() == expected.len()
        && path
            .path
            .segments
            .iter()
            .zip(expected)
            .all(|(segment, expected)| segment.ident == *expected)
}

fn function_present(file: &syn::File, name: &str, public: bool) -> bool {
    let mut visitor = FunctionVisitor { name, public, found: false };
    visitor.visit_file(file);
    visitor.found
}

#[derive(Clone, Copy)]
enum TypeKind {
    Struct,
    Enum,
}

fn type_present(file: &syn::File, name: &str, kind: TypeKind) -> bool {
    let mut visitor = TypeVisitor { name, kind, found: false };
    visitor.visit_file(file);
    visitor.found
}

struct FunctionVisitor<'a> {
    name: &'a str,
    public: bool,
    found: bool,
}

impl<'ast> Visit<'ast> for FunctionVisitor<'_> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if node.sig.ident == self.name
            && (!self.public || matches!(&node.vis, syn::Visibility::Public(_)))
        {
            self.found = true;
        }
        syn::visit::visit_item_fn(self, node);
    }
}

struct TypeVisitor<'a> {
    name: &'a str,
    kind: TypeKind,
    found: bool,
}

impl<'ast> Visit<'ast> for TypeVisitor<'_> {
    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        if matches!(self.kind, TypeKind::Struct)
            && node.ident == self.name
            && matches!(&node.vis, syn::Visibility::Public(_))
        {
            self.found = true;
        }
        syn::visit::visit_item_struct(self, node);
    }

    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        if matches!(self.kind, TypeKind::Enum)
            && node.ident == self.name
            && matches!(&node.vis, syn::Visibility::Public(_))
        {
            self.found = true;
        }
        syn::visit::visit_item_enum(self, node);
    }
}

struct PathVisitor<'a> {
    expected: Vec<&'a str>,
    found: bool,
}

impl<'ast> Visit<'ast> for PathVisitor<'_> {
    fn visit_path(&mut self, node: &'ast syn::Path) {
        if node.segments.len() == self.expected.len()
            && node
                .segments
                .iter()
                .zip(&self.expected)
                .all(|(segment, expected)| segment.ident == *expected)
        {
            self.found = true;
        }
        syn::visit::visit_path(self, node);
    }
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
    if contract.components.is_empty() {
        bail!(
            "probe contract for {} declares no components; an empty positive surface cannot establish presence",
            contract.node_id
        );
    }

    let mut met = 0usize;
    let mut unmet: Vec<String> = Vec::new();
    for component in contract.components {
        let mut satisfied = true;
        for selector in component.selectors {
            let Some(text) = tree.read_text(selector.path)? else {
                satisfied = false;
                break;
            };
            if !selector.anchors.iter().all(|anchor| semantic_anchor_present(&text, anchor)) {
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

/// Node limitation recorded when implementation presence was probed from a
/// tree other than the one an observation describes (#11626, raised by review
/// on #15094).
///
/// The offline projection answers "what is implemented **on this tree**". A
/// consumer that joins it to an observation of a different revision — a stored
/// fixture, most obviously, whose recorded head is synthetic — would otherwise
/// present actions from one revision beside implementation states from another
/// with nothing marking the seam.
pub const PROBED_FROM_A_DIFFERENT_TREE: &str = "c02_implementation_probed_from_a_different_tree";
