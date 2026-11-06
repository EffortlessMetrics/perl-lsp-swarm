//! Typed Docker workspace path / mount contract (#8987).
//!
//! Builds and validates container launch plans so client-visible file URIs and
//! container filesystem paths share one namespace for supported topologies.
//! This module does not rewrite URIs.

mod contract;
mod plan;

pub use contract::{
    HostEditorLauncherSpec, MultiRootPolicy, PathTopology, PlatformCellId, load_contract,
};
pub use plan::{
    BindMount, DockerLaunchPlan, PlanError, WorkspaceFolder, build_host_editor_plan,
    build_inside_container_plan, reject_lossy_host_remap, uri_to_workspace_path,
    validate_documentation_pointers,
};
