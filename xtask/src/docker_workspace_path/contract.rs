use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const CONTRACT_REL_PATH: &str = "policy/docker-workspace-path.v1.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PathTopology {
    EditorInsideContainer,
    HostEditorSamePathBind,
}

impl PathTopology {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EditorInsideContainer => "editor_inside_container",
            Self::HostEditorSamePathBind => "host_editor_same_path_bind",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        match id {
            "editor_inside_container" => Some(Self::EditorInsideContainer),
            "host_editor_same_path_bind" => Some(Self::HostEditorSamePathBind),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformCellId {
    LinuxHostEditor,
    MacOsHostEditor,
    WindowsWslEditor,
    EditorInContainer,
    NativeWindowsHostEditor,
}

impl PlatformCellId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LinuxHostEditor => "linux_host_editor",
            Self::MacOsHostEditor => "macos_host_editor",
            Self::WindowsWslEditor => "windows_wsl_editor",
            Self::EditorInContainer => "editor_in_container",
            Self::NativeWindowsHostEditor => "native_windows_host_editor",
        }
    }
}

#[derive(Debug, Clone)]
pub struct HostEditorLauncherSpec {
    pub image_env: String,
    pub default_image: String,
    pub entrypoint: String,
    pub default_args: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiRootPolicy {
    RefuseIfMultipleRoots,
}

#[derive(Debug)]
pub struct LoadedContract {
    pub topologies: Vec<TopologyRow>,
    pub platform_cells: Vec<PlatformCellRow>,
    pub host_editor_launcher: HostEditorLauncherSpec,
    pub multi_root: MultiRootPolicy,
    pub documentation: DocumentationPointers,
}

#[derive(Debug)]
pub struct DocumentationPointers {
    pub reference: PathBuf,
    pub devcontainer_example: PathBuf,
    pub host_launcher: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawContract {
    schema_version: u32,
    contract: String,
    topology: Vec<RawTopology>,
    platform_cell: Vec<RawPlatformCell>,
    host_editor_launcher: RawHostEditorLauncher,
    multi_root: RawMultiRoot,
    documentation: RawDocumentation,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTopology {
    id: String,
    description: String,
    mount: RawMount,
    launch: RawLaunch,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMount {
    kind: String,
    #[serde(default)]
    container_path: Option<String>,
    read_only_default: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLaunch {
    stdio: bool,
    allocate_tty: bool,
    require_init: bool,
    #[serde(default)]
    explicit_workdir: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPlatformCell {
    id: String,
    same_path_bind_eligible: bool,
    topology: String,
    #[serde(default)]
    supported: Option<bool>,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawHostEditorLauncher {
    image_env: String,
    default_image: String,
    entrypoint: String,
    default_args: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMultiRoot {
    host_wrapper: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDocumentation {
    reference: String,
    devcontainer_example: String,
    host_launcher: String,
}

#[derive(Debug)]
pub struct TopologyRow {
    pub id: PathTopology,
    pub description: String,
    pub mount_kind: String,
    pub container_path: Option<String>,
    pub read_only_default: bool,
    pub launch_stdio: bool,
    pub launch_allocate_tty: bool,
    pub launch_require_init: bool,
    pub launch_explicit_workdir: bool,
}

#[derive(Debug)]
pub struct PlatformCellRow {
    pub id: PlatformCellId,
    pub same_path_bind_eligible: bool,
    pub topology: PathTopology,
    pub supported: bool,
    pub note: Option<String>,
}

pub fn load_contract(repo_root: &Path) -> Result<LoadedContract> {
    let path = repo_root.join(CONTRACT_REL_PATH);
    let raw_text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading docker workspace path contract {}", path.display()))?;
    let raw: RawContract = toml::from_str(&raw_text)
        .with_context(|| format!("parsing docker workspace path contract {}", path.display()))?;
    if raw.schema_version != 1 {
        bail!(
            "unsupported docker workspace path schema version {}; expected 1",
            raw.schema_version
        );
    }
    if raw.contract != "docker_workspace_path.v1" {
        bail!(
            "unexpected docker workspace path contract id {}; expected docker_workspace_path.v1",
            raw.contract
        );
    }

    let mut topologies = Vec::new();
    for row in raw.topology {
        let id = PathTopology::parse(&row.id)
            .ok_or_else(|| anyhow::anyhow!("unknown topology id {}", row.id))?;
        topologies.push(TopologyRow {
            id,
            description: row.description,
            mount_kind: row.mount.kind,
            container_path: row.mount.container_path,
            read_only_default: row.mount.read_only_default,
            launch_stdio: row.launch.stdio,
            launch_allocate_tty: row.launch.allocate_tty,
            launch_require_init: row.launch.require_init,
            launch_explicit_workdir: row.launch.explicit_workdir,
        });
    }
    topologies.sort_by_key(|row| row.id);

    let mut platform_cells = Vec::new();
    for row in raw.platform_cell {
        let id = parse_platform_cell(&row.id)?;
        let topology = PathTopology::parse(&row.topology)
            .ok_or_else(|| anyhow::anyhow!("unknown platform topology {}", row.topology))?;
        platform_cells.push(PlatformCellRow {
            id,
            same_path_bind_eligible: row.same_path_bind_eligible,
            topology,
            supported: row.supported.unwrap_or(true),
            note: row.note,
        });
    }

    let multi_root = match raw.multi_root.host_wrapper.as_str() {
        "refuse_if_multiple_roots" => MultiRootPolicy::RefuseIfMultipleRoots,
        other => bail!("unsupported multi_root.host_wrapper {other}"),
    };

    Ok(LoadedContract {
        topologies,
        platform_cells,
        host_editor_launcher: HostEditorLauncherSpec {
            image_env: raw.host_editor_launcher.image_env,
            default_image: raw.host_editor_launcher.default_image,
            entrypoint: raw.host_editor_launcher.entrypoint,
            default_args: raw.host_editor_launcher.default_args,
        },
        multi_root,
        documentation: DocumentationPointers {
            reference: PathBuf::from(raw.documentation.reference),
            devcontainer_example: PathBuf::from(raw.documentation.devcontainer_example),
            host_launcher: PathBuf::from(raw.documentation.host_launcher),
        },
    })
}

fn parse_platform_cell(id: &str) -> Result<PlatformCellId> {
    Ok(match id {
        "linux_host_editor" => PlatformCellId::LinuxHostEditor,
        "macos_host_editor" => PlatformCellId::MacOsHostEditor,
        "windows_wsl_editor" => PlatformCellId::WindowsWslEditor,
        "editor_in_container" => PlatformCellId::EditorInContainer,
        "native_windows_host_editor" => PlatformCellId::NativeWindowsHostEditor,
        other => bail!("unknown platform_cell id {other}"),
    })
}
