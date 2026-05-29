mod metadata;
mod metrics;
mod report;

use color_eyre::eyre::{Result, WrapErr};
use std::fs;
use std::path::PathBuf;

pub fn run(output: Option<PathBuf>) -> Result<()> {
    let metadata = metadata::cargo_metadata()?;
    let (workspace_root, metrics) = metrics::collect(metadata)?;
    let output = output.unwrap_or_else(|| workspace_root.join("docs/SRP_MICROCRATES.md"));
    let report = report::render(&metrics);

    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .wrap_err_with(|| format!("failed to create output directory: {}", parent.display()))?;
    }
    fs::write(&output, report)
        .wrap_err_with(|| format!("failed to write report to {}", output.display()))?;

    println!("Wrote SRP report to {}", output.display());
    Ok(())
}
