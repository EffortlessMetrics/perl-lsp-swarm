//! Architecture boundary for the transport-neutral workspace-runtime generation core.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};

const MODULE_PATH: &str = "crates/perl-workspace/src/workspace/runtime_generation.rs";
const CORE_PATH: &str = "crates/perl-workspace/src/workspace/runtime_generation/core.rs";
const MODULE_ROOT_PATH: &str = "crates/perl-workspace/src/workspace/mod.rs";

const REQUIRED_WRAPPER_MARKERS: &[&str] = &[
    "mod core;",
    "pub struct WorkspaceRuntimeController",
    "lifecycle_gate: Arc<RwLock<()>>",
    "let _lifecycle = self.lifecycle_gate.read();",
    "let _lifecycle = self.lifecycle_gate.write();",
    "WorkspaceRuntimeLifecycleState::Detached | WorkspaceRuntimeLifecycleState::Shutdown",
];

const REQUIRED_CORE_MARKERS: &[&str] = &[
    "pub struct WorkspaceRuntimeGeneration",
    "pub(crate) struct WorkspaceRuntimeController",
    "pub fn mint() -> Self",
    "pub(crate) fn begin_transition",
    "pub(crate) fn register_root_task",
    "pub(crate) fn accept_publication",
    "pub(crate) fn detach_root",
    "operation_id: WorkspaceRuntimeOperationId",
    "shutdown: AtomicBool",
    "roots: RwLock<BTreeMap<WorkspaceRootId, Arc<Mutex<RootEntry>>>>",
];

const FORBIDDEN_TOKENS: &[&str] = &[
    "LspServer",
    "lsp_types",
    "WorkspaceEdit",
    "workspace/applyEdit",
    "rmcp",
    "SnapshotStore",
    "ProjectModel",
    "tokio::",
    "std::path",
    "url::",
    "serde::Serialize",
    "serde::Deserialize",
    "inner: Arc<Mutex<ControllerState>>",
    "WorkspaceRuntimeSessionId::new(",
];

fn repo_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask must live immediately beneath the repository root")
}

fn read_source(root: &Path, path: &str) -> Result<String> {
    fs::read_to_string(root.join(path)).with_context(|| format!("read {path}"))
}

fn forbidden_tokens(source: &str) -> Vec<&'static str> {
    FORBIDDEN_TOKENS.iter().copied().filter(|token| source.contains(token)).collect()
}

#[test]
fn workspace_runtime_generation_authority_is_transport_and_domain_neutral() -> Result<()> {
    let root = repo_root()?;
    let wrapper = read_source(&root, MODULE_PATH)?;
    let core = read_source(&root, CORE_PATH)?;
    let combined = format!("{wrapper}\n{core}");

    let forbidden = forbidden_tokens(&combined);
    ensure!(
        forbidden.is_empty(),
        "workspace-runtime authority contains forbidden transport/domain/global-lifecycle dependencies: {forbidden:?}"
    );

    for marker in REQUIRED_WRAPPER_MARKERS {
        ensure!(
            wrapper.contains(marker),
            "{MODULE_PATH} is missing wrapper authority marker {marker:?}"
        );
    }
    for marker in REQUIRED_CORE_MARKERS {
        ensure!(core.contains(marker), "{CORE_PATH} is missing generation-core marker {marker:?}");
    }

    ensure!(
        !wrapper.contains("pub mod core;"),
        "{MODULE_PATH} must keep the implementation core private"
    );
    Ok(())
}

#[test]
fn workspace_runtime_generation_module_is_hidden_and_not_glob_reexported() -> Result<()> {
    let root = repo_root()?;
    let source = read_source(&root, MODULE_ROOT_PATH)?;
    let normalized = source.replace("\r\n", "\n");

    ensure!(
        normalized.contains("#[doc(hidden)]\npub mod runtime_generation;"),
        "{MODULE_ROOT_PATH} must expose the cross-crate integration module as doc-hidden"
    );
    ensure!(
        normalized
            .contains("/// Process-local root-generation and publication-eligibility authority."),
        "{MODULE_ROOT_PATH} must keep the integration module's authority declaration"
    );
    ensure!(
        !source.contains("pub use runtime_generation"),
        "{MODULE_ROOT_PATH} must not add the v0.x integration types to the curated re-export surface"
    );

    Ok(())
}

#[test]
fn a_transport_or_semantic_back_edge_is_rejected() {
    let mutated = r#"
        use lsp_types::WorkspaceEdit;
        struct WorkspaceRuntimeGeneration;
    "#;

    let forbidden = forbidden_tokens(mutated);
    assert!(forbidden.contains(&"lsp_types"), "LSP wire types must be rejected");
    assert!(forbidden.contains(&"WorkspaceEdit"), "edit semantics must be rejected");
}

#[test]
fn a_path_owned_generation_core_is_rejected() {
    let mutated = r#"
        use std::path::PathBuf;
        struct WorkspaceRuntimeGeneration {
            root: PathBuf,
        }
    "#;

    let forbidden = forbidden_tokens(mutated);
    assert!(
        forbidden.contains(&"std::path"),
        "path equality must not become runtime-generation identity"
    );
}

#[test]
fn a_global_controller_lifecycle_mutex_is_rejected() {
    let mutated = r#"
        struct WorkspaceRuntimeController {
            inner: Arc<Mutex<ControllerState>>,
        }
    "#;

    let forbidden = forbidden_tokens(mutated);
    assert!(
        forbidden.contains(&"inner: Arc<Mutex<ControllerState>>"),
        "unrelated roots must not share one exclusive lifecycle mutex"
    );
}

#[test]
fn shutdown_gate_requires_shared_admission_and_exclusive_close() -> Result<()> {
    let root = repo_root()?;
    let wrapper = read_source(&root, MODULE_PATH)?;

    // Bind the read-gate requirement to the wrapper's public method surface:
    // the constructor builds the gate and `shutdown` takes the write gate, so
    // every remaining public admission/observation method must hold the read
    // gate. A floor count would keep passing when an ungated method is added.
    let public_methods = wrapper.matches("    pub fn ").count();
    let read_gates = wrapper.matches("let _lifecycle = self.lifecycle_gate.read();").count();
    ensure!(
        public_methods >= 3 && read_gates == public_methods - 2,
        "every public controller method except the constructor and shutdown must hold the \
         admission read gate (found {read_gates} read gates across {public_methods} public methods)"
    );
    ensure!(
        wrapper.matches("let _lifecycle = self.lifecycle_gate.write();").count() == 1,
        "shutdown must be the only exclusive application-lifecycle transition"
    );
    Ok(())
}

#[test]
fn generation_allocation_shares_the_installation_linearization_point() -> Result<()> {
    let root = repo_root()?;
    let core = read_source(&root, CORE_PATH)?;

    // Same-root transitions must mint their generation under the root-map
    // write guard so allocation order and installation order cannot diverge.
    let guard = core.find("let mut roots = self.inner.roots.write();");
    let allocation = core.find("self.allocate_generation(root_id)?");
    ensure!(
        guard.is_some_and(|guard| allocation.is_some_and(|allocation| guard < allocation)),
        "{CORE_PATH} must allocate each generation inside the root-map write guard"
    );
    Ok(())
}
