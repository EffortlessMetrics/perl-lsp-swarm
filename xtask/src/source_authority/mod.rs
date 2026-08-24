//! Source-authority and instruction/data boundary for Zed agent stage
//! packets.
//!
//! Every file inside the stage-packet tree must carry an explicit authority
//! classification, a currentness digest, and a derived instruction capability.
//! Repository policy and maintainer rulings are the only directive classes;
//! bot comments, external sources, logs, tool output, and outbound rendered
//! bodies are data to inspect, never instructions to execute. The verifier
//! fails closed on unclassified content, stale digests, superseded rulings,
//! conflicting authorities, and undeclared packet generators.

mod model;
mod verify;

pub use model::{
    EXTERNAL_WRITE_POLICY, PacketInput, RulingBinding, SOURCE_AUTHORITY_SCHEMA_VERSION,
    Sensitivity, SourceAuthorityClass, SourceAuthorityManifest, normalize_content,
    normalized_digest,
};
pub use verify::{Receipt, Violation, verify_manifest};

use clap::Parser;
use color_eyre::eyre::{Result, WrapErr, bail};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

#[derive(Debug, Parser)]
#[command(about = "Verify Zed stage-packet source-authority boundaries")]
struct Args {
    /// Source-authority manifest JSON.
    fixture: PathBuf,

    /// Repository root used to resolve the packet-relative subjects.
    #[arg(long, default_value = ".")]
    repo_root: PathBuf,

    /// Receipt JSON retained for clean and blocking verdicts.
    #[arg(long, default_value = "target/receipts/zed-source-authority.json")]
    out: PathBuf,
}

pub fn run_from_env() -> Result<()> {
    let args = Args::parse();
    run_with_paths(args.fixture, args.repo_root, args.out)
}

pub fn run_with_paths(fixture: PathBuf, repo_root: PathBuf, out: PathBuf) -> Result<()> {
    let raw = fs::read_to_string(&fixture)
        .wrap_err_with(|| format!("reading source-authority fixture {}", fixture.display()))?;
    let manifest: SourceAuthorityManifest = serde_json::from_str(&raw)
        .wrap_err_with(|| format!("parsing source-authority fixture {}", fixture.display()))?;

    let receipt = verify_manifest(&manifest, &repo_root)?;
    write_receipt(&out, &receipt)?;

    if receipt.violations.is_empty() {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        writeln!(
            handle,
            "zed-train source-check: clean; {} classified inputs, {} declared generators",
            receipt.checked_inputs, receipt.checked_generators
        )?;
        return Ok(());
    }

    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    for violation in &receipt.violations {
        writeln!(
            handle,
            "violation[{}] {}: {}",
            violation.code, violation.subject, violation.detail
        )?;
    }
    bail!(
        "zed-train source-check: stage-packet authority boundary is not proven; see {}",
        out.display()
    )
}

fn write_receipt(path: &Path, receipt: &Receipt) -> Result<()> {
    let parent =
        path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs::create_dir_all(parent)
        .wrap_err_with(|| format!("creating source-authority receipt dir {}", parent.display()))?;
    let raw = serde_json::to_string_pretty(receipt).wrap_err("serializing authority receipt")?;
    let mut temporary = NamedTempFile::new_in(parent).wrap_err_with(|| {
        format!("creating atomic source-authority receipt in {}", parent.display())
    })?;
    writeln!(temporary, "{raw}").wrap_err_with(|| {
        format!("writing temporary source-authority receipt for {}", path.display())
    })?;
    temporary.persist(path).map_err(|error| {
        color_eyre::eyre::eyre!(
            "atomically persisting source-authority receipt {}: {}",
            path.display(),
            error.error
        )
    })?;
    Ok(())
}
