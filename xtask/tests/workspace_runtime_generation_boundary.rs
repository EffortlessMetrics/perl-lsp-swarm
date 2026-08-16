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
    "pub struct WorkspaceRuntimeController",
    "pub fn begin_transition",
    "pub fn register_root_task",
    "pub fn accept_publication",
    "pub fn detach_root",
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
    FORBIDDEN_TOKENS
        .iter()
        .copied()
        .filter(|token| source.contains(token))
        .collect()
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
        ensure!(
            core.contains(marker),
            "{CORE_PATH} is missing generation-core marker {marker:?}"
        );
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

    ensure!(
        source.contains(
            "/// Process-local root-generation and publication-eligibility authority.\n#[doc(hidden)]\npub mod runtime_generation;"
        ),
        "{MODULE_ROOT_PATH} must expose the cross-crate integration module as doc-hidden"
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
    assert!(
        forbidden.contains(&"lsp_types"),
        "LSP wire types must be rejected"
    );
    assert!(
        forbidden.contains(&"WorkspaceEdit"),
        "edit semantics must be rejected"
    );
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

    ensure!(
        wrapper.matches("let _lifecycle = self.lifecycle_gate.read();").count() >= 10,
        "all root admission and observation paths must share the shutdown gate"
    );
    ensure!(
        wrapper.matches("let _lifecycle = self.lifecycle_gate.write();").count() == 1,
        "shutdown must be the only exclusive application-lifecycle transition"
    );
    Ok(())
}
