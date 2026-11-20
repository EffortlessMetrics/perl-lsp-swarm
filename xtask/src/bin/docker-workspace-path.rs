//! CLI for the Docker workspace path contract (#8987).

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

use xtask::docker_workspace_path::{
    WorkspaceFolder, build_host_editor_plan, build_inside_container_plan, load_contract,
    validate_documentation_pointers,
};

#[derive(Parser, Debug)]
#[command(about = "Plan Docker launches that preserve workspace path identity")]
struct Args {
    #[arg(long, default_value = ".")]
    root: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Validate policy pointers and embedded documentation surfaces exist.
    Check,
    /// Print a `docker run` argv for the host-editor same-path topology.
    HostEditor {
        /// Absolute workspace root path (must match client rootUri).
        #[arg(long)]
        workspace: PathBuf,
        /// Optional image override (defaults to contract default).
        #[arg(long)]
        image: Option<String>,
    },
    /// Print a `docker run` argv for editor-inside-container (/workspace) topology.
    InsideContainer {
        #[arg(long)]
        host_path: PathBuf,
        #[arg(long)]
        image: Option<String>,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Command::Check => {
            let contract = load_contract(&args.root).context("load contract")?;
            validate_documentation_pointers(&args.root).context("documentation pointers")?;
            println!(
                "docker-workspace-path: OK ({} topologies, default image {})",
                contract.topologies.len(),
                contract.host_editor_launcher.default_image
            );
        }
        Command::HostEditor { workspace, image } => {
            let contract = load_contract(&args.root).context("load contract")?;
            let uri = file_uri(&workspace)?;
            let plan = build_host_editor_plan(
                &contract,
                &[WorkspaceFolder { uri, absolute_path: workspace }],
                image.as_deref(),
            )
            .map_err(|error| anyhow::anyhow!("{error}"))?;
            print_argv(&plan.docker_run_argv());
        }
        Command::InsideContainer { host_path, image } => {
            let contract = load_contract(&args.root).context("load contract")?;
            let plan = build_inside_container_plan(&contract, &host_path, image.as_deref())
                .map_err(|error| anyhow::anyhow!("{error}"))?;
            print_argv(&plan.docker_run_argv());
        }
    }
    Ok(())
}

fn file_uri(path: &Path) -> Result<String> {
    url::Url::from_file_path(path).map(|value| value.to_string()).map_err(|()| {
        anyhow::anyhow!("path is not representable as a file URI: {}", path.display())
    })
}

fn print_argv(argv: &[String]) {
    for (index, token) in argv.iter().enumerate() {
        if index > 0 {
            print!(" ");
        }
        if token.contains(' ') {
            print!("{token:?}");
        } else {
            print!("{token}");
        }
    }
    println!();
}
