mod model;
mod observe;
mod render;

pub use model::{
    Observation, ObservationState, PlanSummary, PrMatch, ProposedAction, RepositorySubject,
    WORKTREE_CLEANUP_POLICY_VERSION, WORKTREE_CLEANUP_SCHEMA_VERSION, WorktreeActionKind,
    WorktreeClassification, WorktreeCleanupPlan, WorktreeFacts, WorktreePlanEntry,
};
pub use observe::{InspectOptions, inspect, inspect_with_options};
pub use render::render_human;

use clap::{Parser, Subcommand};
use color_eyre::eyre::{Context, Result};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

#[derive(Debug, Parser)]
#[command(about = "Inspect or apply bounded worktree cleanup plans")]
struct Args {
    #[command(subcommand)]
    command: WorktreeCleanupCommand,
}

#[derive(Debug, Subcommand)]
enum WorktreeCleanupCommand {
    /// Observe and classify registered worktrees without changing repository state.
    Inspect {
        /// Repository or worktree path to inspect.
        #[arg(long, default_value = ".")]
        root: PathBuf,

        /// Emit the complete typed plan as JSON on stdout.
        #[arg(long)]
        json: bool,

        /// Write the complete typed JSON plan to this explicit path.
        #[arg(long = "json-out")]
        json_out: Option<PathBuf>,
    },
}

pub fn run_from_env() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    run(args)
}

fn run(args: Args) -> Result<()> {
    match args.command {
        WorktreeCleanupCommand::Inspect { root, json, json_out } => {
            let plan = inspect(&root)?;
            let json_text = format!(
                "{}\n",
                serde_json::to_string_pretty(&plan)
                    .wrap_err("serializing worktree cleanup plan")?
            );
            if let Some(path) = json_out {
                write_atomic(&path, json_text.as_bytes())?;
            }
            let stdout_text = if json { json_text } else { render_human(&plan) };
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            handle
                .write_all(stdout_text.as_bytes())
                .wrap_err("writing worktree cleanup inspection")?;
            Ok(())
        }
    }
}

fn write_atomic(path: &Path, content: &[u8]) -> Result<()> {
    let parent =
        path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs::create_dir_all(parent).wrap_err_with(|| {
        format!("creating worktree plan output directory {}", parent.display())
    })?;
    let mut temporary = NamedTempFile::new_in(parent)
        .wrap_err_with(|| format!("creating temporary worktree plan in {}", parent.display()))?;
    temporary
        .write_all(content)
        .wrap_err_with(|| format!("writing temporary worktree plan for {}", path.display()))?;
    temporary
        .as_file_mut()
        .sync_all()
        .wrap_err_with(|| format!("syncing temporary worktree plan for {}", path.display()))?;
    temporary.persist(path).map_err(|error| {
        color_eyre::eyre::eyre!(
            "atomically persisting worktree plan {}: {}",
            path.display(),
            error.error
        )
    })?;
    Ok(())
}
