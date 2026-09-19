mod authority;
mod classify;
mod model;
mod path;

#[cfg(test)]
mod authorization_tests;
#[cfg(test)]
mod schema_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;

use crate::receipt_output::{ensure_safe_output, prepare_output_parent, write_receipt};
use authority::load_authority;
use clap::Parser;
use classify::classify;
use color_eyre::eyre::{Result, WrapErr, bail};
use model::{Observation, Verdict};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Label used in receipt-output diagnostics for this classifier.
const SUBJECT: &str = "publication drift";

#[derive(Debug, Parser)]
#[command(about = "Classify an exact-SHA publication drift observation")]
struct Args {
    /// Comparison observation JSON.
    #[arg(long)]
    input: PathBuf,

    /// Repository root used to resolve the authority manifest's repository-relative path.
    #[arg(long, default_value = ".")]
    repo_root: PathBuf,

    /// Receipt JSON written even when the verdict blocks promotion.
    #[arg(long, default_value = "target/receipts/publication-drift.json")]
    out: PathBuf,
}

pub fn run_from_env() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    run_with_paths(args.input, args.repo_root, args.out)
}

pub fn run_with_paths(input: PathBuf, repo_root: PathBuf, out: PathBuf) -> Result<()> {
    let observation = load_observation(&input)?;
    let authority_path =
        observation.manifest.as_ref().map(|manifest| repo_root.join(&manifest.path));
    prepare_output_parent(SUBJECT, &out)?;
    let mut protected: Vec<&Path> = vec![input.as_path()];
    if let Some(authority_path) = authority_path.as_deref() {
        protected.push(authority_path);
    }
    ensure_safe_output(SUBJECT, &out, &protected)?;

    let authority = load_authority(&repo_root, observation.manifest.as_ref());
    let receipt = classify(observation, authority);
    write_receipt(SUBJECT, &out, &receipt)?;

    match receipt.verdict {
        Verdict::Clean => {
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            writeln!(
                handle,
                "publication-drift: clean comparison {} -> {} at version {}",
                receipt.swarm.sha,
                receipt.public.sha,
                receipt.comparison_version.as_deref().unwrap_or("not-proven")
            )?;
            Ok(())
        }
        Verdict::Drift => bail!("publication-drift: product drift detected; see {}", out.display()),
        Verdict::NotProven => {
            bail!("publication-drift: comparison not proven; see {}", out.display())
        }
    }
}

fn load_observation(path: &Path) -> Result<Observation> {
    let raw = fs::read_to_string(path)
        .wrap_err_with(|| format!("reading publication drift observation {}", path.display()))?;
    serde_json::from_str(&raw)
        .wrap_err_with(|| format!("parsing publication drift observation {}", path.display()))
}
