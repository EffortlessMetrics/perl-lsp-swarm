mod authority;
mod classify;
mod model;
mod path;

#[cfg(test)]
mod authorization_tests;
#[cfg(test)]
mod schema_tests;
#[cfg(test)]
mod tests;

use authority::load_authority;
use clap::Parser;
use classify::classify;
use color_eyre::eyre::{Result, WrapErr, bail};
use model::{Observation, Receipt, Verdict};
use std::fs;
use std::path::{Path, PathBuf};

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
    let observation = load_observation(&args.input)?;
    let authority = load_authority(&args.repo_root, observation.manifest.as_ref());
    let receipt = classify(observation, authority);
    write_receipt(&args.out, &receipt)?;

    match receipt.verdict {
        Verdict::Clean => {
            println!(
                "publication-drift: clean comparison {} -> {} at version {}",
                receipt.swarm.sha,
                receipt.public.sha,
                receipt.comparison_version.as_deref().unwrap_or("not-proven")
            );
            Ok(())
        }
        Verdict::Drift => bail!(
            "publication-drift: product drift detected; see {}",
            args.out.display()
        ),
        Verdict::NotProven => bail!(
            "publication-drift: comparison not proven; see {}",
            args.out.display()
        ),
    }
}

fn load_observation(path: &Path) -> Result<Observation> {
    let raw = fs::read_to_string(path)
        .wrap_err_with(|| format!("reading publication drift observation {}", path.display()))?;
    serde_json::from_str(&raw)
        .wrap_err_with(|| format!("parsing publication drift observation {}", path.display()))
}

fn write_receipt(path: &Path, receipt: &Receipt) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .wrap_err_with(|| format!("creating publication drift output {}", parent.display()))?;
    }
    let raw = serde_json::to_string_pretty(receipt).wrap_err("serializing drift receipt")?;
    fs::write(path, format!("{raw}\n"))
        .wrap_err_with(|| format!("writing publication drift receipt {}", path.display()))
}
