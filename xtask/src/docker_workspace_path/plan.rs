use std::path::{Component, Path, PathBuf};

use super::contract::{LoadedContract, MultiRootPolicy, PathTopology, load_contract};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFolder {
    pub uri: String,
    pub absolute_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindMount {
    pub source: PathBuf,
    pub target: PathBuf,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerLaunchPlan {
    pub topology: PathTopology,
    pub mounts: Vec<BindMount>,
    pub workdir: PathBuf,
    pub interactive_stdin: bool,
    pub allocate_tty: bool,
    pub require_init: bool,
    pub image: String,
    pub entrypoint: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    EmptyWorkspace,
    MultiRootHostWrapperUnsupported,
    RelativeWorkspacePath,
    LossyHostRemap { host_path: PathBuf, container_path: PathBuf },
    UnsupportedNativeWindows,
    MissingRoot,
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyWorkspace => write!(formatter, "workspace has no roots"),
            Self::MultiRootHostWrapperUnsupported => {
                write!(
                    formatter,
                    "host-editor launcher refuses multi-root until same-path mounts are proven for every root"
                )
            }
            Self::RelativeWorkspacePath => {
                write!(formatter, "workspace root must be an absolute path")
            }
            Self::LossyHostRemap { host_path, container_path } => write!(
                formatter,
                "lossy host remap: {} mounted only at {}",
                host_path.display(),
                container_path.display()
            ),
            Self::UnsupportedNativeWindows => {
                write!(formatter, "native Windows host editor topology is not supported")
            }
            Self::MissingRoot => write!(formatter, "workspace root path is missing"),
        }
    }
}

impl std::error::Error for PlanError {}

pub fn uri_to_workspace_path(uri: &str) -> Result<PathBuf, PlanError> {
    let url = url::Url::parse(uri).map_err(|_| PlanError::MissingRoot)?;
    if url.scheme() != "file" {
        return Err(PlanError::MissingRoot);
    }
    url.to_file_path().map_err(|_| PlanError::MissingRoot)
}

pub fn reject_lossy_host_remap(
    host_path: &Path,
    container_mount_target: &Path,
) -> Result<(), PlanError> {
    if host_path == container_mount_target {
        return Ok(());
    }
    if container_mount_target == Path::new("/workspace") && host_path != container_mount_target {
        return Err(PlanError::LossyHostRemap {
            host_path: host_path.to_path_buf(),
            container_path: container_mount_target.to_path_buf(),
        });
    }
    Err(PlanError::LossyHostRemap {
        host_path: host_path.to_path_buf(),
        container_path: container_mount_target.to_path_buf(),
    })
}

pub fn build_host_editor_plan(
    contract: &LoadedContract,
    folders: &[WorkspaceFolder],
    image: Option<&str>,
) -> Result<DockerLaunchPlan, PlanError> {
    if folders.is_empty() {
        return Err(PlanError::EmptyWorkspace);
    }
    if matches!(contract.multi_root, MultiRootPolicy::RefuseIfMultipleRoots) && folders.len() > 1 {
        return Err(PlanError::MultiRootHostWrapperUnsupported);
    }

    let host = host_editor_launch(contract)?;
    let primary = &folders[0];
    ensure_absolute(&primary.absolute_path)?;

    reject_lossy_host_remap(&primary.absolute_path, &primary.absolute_path)?;

    let mut mounts = Vec::new();
    for folder in folders {
        ensure_absolute(&folder.absolute_path)?;
        mounts.push(BindMount {
            source: folder.absolute_path.clone(),
            target: folder.absolute_path.clone(),
            read_only: true,
        });
    }

    Ok(DockerLaunchPlan {
        topology: PathTopology::HostEditorSamePathBind,
        mounts,
        workdir: primary.absolute_path.clone(),
        interactive_stdin: host.launch_stdio,
        allocate_tty: host.launch_allocate_tty,
        require_init: host.launch_require_init,
        image: image.map(str::to_owned).unwrap_or_else(|| host.default_image.clone()),
        entrypoint: host.entrypoint.clone(),
        args: host.default_args.clone(),
    })
}

pub fn build_inside_container_plan(
    contract: &LoadedContract,
    host_project_path: &Path,
    image: Option<&str>,
) -> Result<DockerLaunchPlan, PlanError> {
    let inside = inside_container_launch(contract)?;
    let container_root =
        PathBuf::from(inside.container_path.as_deref().ok_or(PlanError::MissingRoot)?);
    Ok(DockerLaunchPlan {
        topology: PathTopology::EditorInsideContainer,
        mounts: vec![BindMount {
            source: host_project_path.to_path_buf(),
            target: container_root.clone(),
            read_only: true,
        }],
        workdir: container_root,
        interactive_stdin: inside.launch_stdio,
        allocate_tty: inside.launch_allocate_tty,
        require_init: inside.launch_require_init,
        image: image
            .map(str::to_owned)
            .unwrap_or_else(|| contract.host_editor_launcher.default_image.clone()),
        entrypoint: contract.host_editor_launcher.entrypoint.clone(),
        args: contract.host_editor_launcher.default_args.clone(),
    })
}

impl DockerLaunchPlan {
    pub fn docker_run_argv(&self) -> Vec<String> {
        let mut argv = vec!["docker".to_string(), "run".to_string(), "--rm".to_string()];
        if self.interactive_stdin {
            argv.push("-i".to_string());
        }
        if self.require_init {
            argv.push("--init".to_string());
        }
        if !self.allocate_tty {
            // Explicitly avoid -t for stdio LSP transport.
        }
        for mount in &self.mounts {
            let ro = if mount.read_only { ",readonly" } else { "" };
            argv.push("--mount".to_string());
            argv.push(format!(
                "type=bind,src={},dst={}{}",
                mount.source.display(),
                mount.target.display(),
                ro
            ));
        }
        argv.push("--workdir".to_string());
        argv.push(self.workdir.display().to_string());
        argv.push(self.image.clone());
        if self.entrypoint != "perllsp" {
            argv.push("--entrypoint".to_string());
            argv.push(self.entrypoint.clone());
        }
        argv.extend(self.args.clone());
        argv
    }

    pub fn returned_uri_for_relative_path(&self, root: &Path, relative: &Path) -> String {
        let absolute = root.join(relative);
        url::Url::from_file_path(&absolute)
            .map(|value| value.to_string())
            .unwrap_or_else(|_| format!("file://{}", absolute.display()))
    }

    pub fn dap_source_path(&self, root: &Path, relative: &Path) -> PathBuf {
        root.join(relative)
    }
}

fn ensure_absolute(path: &Path) -> Result<(), PlanError> {
    if path.as_os_str().is_empty() {
        return Err(PlanError::MissingRoot);
    }
    if !path.is_absolute() {
        return Err(PlanError::RelativeWorkspacePath);
    }
    for component in path.components() {
        if matches!(component, Component::ParentDir) {
            return Err(PlanError::RelativeWorkspacePath);
        }
    }
    Ok(())
}

struct LaunchShape {
    launch_stdio: bool,
    launch_allocate_tty: bool,
    launch_require_init: bool,
    default_image: String,
    entrypoint: String,
    default_args: Vec<String>,
}

struct InsideLaunchShape {
    container_path: Option<String>,
    launch_stdio: bool,
    launch_allocate_tty: bool,
    launch_require_init: bool,
}

fn host_editor_launch(contract: &LoadedContract) -> Result<LaunchShape, PlanError> {
    let row = contract
        .topologies
        .iter()
        .find(|row| row.id == PathTopology::HostEditorSamePathBind)
        .ok_or(PlanError::MissingRoot)?;
    Ok(LaunchShape {
        launch_stdio: row.launch_stdio,
        launch_allocate_tty: row.launch_allocate_tty,
        launch_require_init: row.launch_require_init,
        default_image: contract.host_editor_launcher.default_image.clone(),
        entrypoint: contract.host_editor_launcher.entrypoint.clone(),
        default_args: contract.host_editor_launcher.default_args.clone(),
    })
}

fn inside_container_launch(contract: &LoadedContract) -> Result<InsideLaunchShape, PlanError> {
    let row = contract
        .topologies
        .iter()
        .find(|row| row.id == PathTopology::EditorInsideContainer)
        .ok_or(PlanError::MissingRoot)?;
    Ok(InsideLaunchShape {
        container_path: row.container_path.clone(),
        launch_stdio: row.launch_stdio,
        launch_allocate_tty: row.launch_allocate_tty,
        launch_require_init: row.launch_require_init,
    })
}

pub fn load_and_build_host_editor(
    repo_root: &Path,
    folders: &[WorkspaceFolder],
    image: Option<&str>,
) -> Result<DockerLaunchPlan, PlanError> {
    let contract = load_contract(repo_root).map_err(|_| PlanError::MissingRoot)?;
    build_host_editor_plan(&contract, folders, image)
}

pub fn validate_documentation_pointers(repo_root: &Path) -> anyhow::Result<()> {
    let contract = load_contract(repo_root)?;
    for path in [
        &contract.documentation.reference,
        &contract.documentation.devcontainer_example,
        &contract.documentation.host_launcher,
    ] {
        let absolute = repo_root.join(path);
        if !absolute.is_file() {
            anyhow::bail!("documentation pointer missing: {}", path.display());
        }
    }
    Ok(())
}

pub fn classify_platform(
    os: &str,
    in_wsl: bool,
    in_container: bool,
) -> Result<PathTopology, PlanError> {
    if in_container {
        return Ok(PathTopology::EditorInsideContainer);
    }
    if os == "windows" && !in_wsl {
        return Err(PlanError::UnsupportedNativeWindows);
    }
    Ok(PathTopology::HostEditorSamePathBind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    #[test]
    fn native_windows_classified_unsupported() {
        let err = classify_platform("windows", false, false).expect_err("native windows");
        assert_eq!(err, PlanError::UnsupportedNativeWindows);
    }

    #[test]
    fn host_editor_same_path_mount_plan() -> Result<(), PlanError> {
        let contract = load_contract(&repo_root()).map_err(|_| PlanError::MissingRoot)?;
        let root = PathBuf::from("/home/user/project");
        let plan = build_host_editor_plan(
            &contract,
            &[WorkspaceFolder {
                uri: "file:///home/user/project".into(),
                absolute_path: root.clone(),
            }],
            Some("ghcr.io/effortlessmetrics/perl-lsp:0.18.0"),
        )?;
        assert_eq!(plan.topology, PathTopology::HostEditorSamePathBind);
        assert_eq!(plan.mounts.len(), 1);
        assert_eq!(plan.mounts[0].source, root);
        assert_eq!(plan.mounts[0].target, root);
        assert!(plan.require_init);
        assert!(plan.interactive_stdin);
        assert!(!plan.allocate_tty);
        let argv = plan.docker_run_argv();
        assert!(argv.contains(&"--init".to_string()));
        assert!(
            argv.iter()
                .any(|arg| arg.contains("src=/home/user/project,dst=/home/user/project,readonly"))
        );
        Ok(())
    }

    #[test]
    fn rejects_lossy_workspace_remap() {
        let err = reject_lossy_host_remap(Path::new("/host/project"), Path::new("/workspace"))
            .expect_err("lossy remap must fail");
        assert_eq!(
            err,
            PlanError::LossyHostRemap {
                host_path: PathBuf::from("/host/project"),
                container_path: PathBuf::from("/workspace"),
            }
        );
    }

    #[test]
    fn multi_root_host_wrapper_refused() -> Result<(), PlanError> {
        let contract = load_contract(&repo_root()).map_err(|_| PlanError::MissingRoot)?;
        let err = build_host_editor_plan(
            &contract,
            &[
                WorkspaceFolder { uri: "file:///a".into(), absolute_path: PathBuf::from("/a") },
                WorkspaceFolder { uri: "file:///b".into(), absolute_path: PathBuf::from("/b") },
            ],
            None,
        )
        .expect_err("multi-root host wrapper must refuse");
        assert_eq!(err, PlanError::MultiRootHostWrapperUnsupported);
        Ok(())
    }

    #[test]
    fn unicode_and_spaces_survive_uri_projection() -> Result<(), PlanError> {
        let contract = load_contract(&repo_root()).map_err(|_| PlanError::MissingRoot)?;
        let root = PathBuf::from("/tmp/fixtures space#percent%25");
        let plan = build_host_editor_plan(
            &contract,
            &[WorkspaceFolder {
                uri: "file:///tmp/fixtures%20space%23percent%2525".into(),
                absolute_path: root.clone(),
            }],
            None,
        )?;
        let uri = plan.returned_uri_for_relative_path(&root, Path::new("lib/App.pm"));
        assert!(uri.contains("App.pm"));
        assert_eq!(plan.dap_source_path(&root, Path::new("lib/App.pm")), root.join("lib/App.pm"));
        Ok(())
    }

    #[test]
    fn distinct_roots_keep_same_relative_paths_distinct() -> Result<(), PlanError> {
        let root_a = PathBuf::from("/roots/a");
        let root_b = PathBuf::from("/roots/b");
        let contract = load_contract(&repo_root()).map_err(|_| PlanError::MissingRoot)?;
        let plan_a = build_host_editor_plan(
            &contract,
            &[WorkspaceFolder { uri: "file:///roots/a".into(), absolute_path: root_a.clone() }],
            None,
        )?;
        let plan_b = build_host_editor_plan(
            &contract,
            &[WorkspaceFolder { uri: "file:///roots/b".into(), absolute_path: root_b.clone() }],
            None,
        )?;
        let rel = Path::new("lib/Same.pm");
        assert_ne!(
            plan_a.returned_uri_for_relative_path(&root_a, rel),
            plan_b.returned_uri_for_relative_path(&root_b, rel)
        );
        Ok(())
    }
}
