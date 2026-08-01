//! Compare same-SHA baseline and safe-ICF macOS release artifacts.
//!
//! This is a measurement instrument, not a build or publication authority. It
//! consumes already-built, already-stripped, release-shaped artifacts and emits
//! a deterministic decision receipt for issue #5432.

#![allow(clippy::print_stderr, clippy::print_stdout)]

use clap::Parser;
use color_eyre::eyre::{Result, bail};
use std::path::{Path, PathBuf};

#[path = "../src/bin/release_artifact_size/measure.rs"]
mod measure;
#[path = "../src/bin/release_artifact_size/model.rs"]
mod model;
#[path = "../src/bin/release_artifact_size/render.rs"]
mod render;

use model::{DecisionPolicy, Receipt};

pub(crate) const CHECK_NAME: &str = "release-artifact-size";
pub(crate) const SCHEMA_VERSION: &str = "release_artifact_size.v1";
pub(crate) const REPOSITORY: &str = "EffortlessMetrics/perl-lsp-swarm";
pub(crate) const CLAIM_BOUNDARY: &str = concat!(
    "same-SHA post-strip size and packaged-binary smoke comparison for perllsp ",
    "and perl-dap on one native macOS target; does not prove startup/RSS ",
    "improvement, other targets, publication, or general release readiness"
);
pub(crate) const SAFE_ICF_RUSTFLAGS: &str =
    "-C linker=rust-lld -C linker-flavor=ld64.lld -C link-arg=--icf=safe";
pub(crate) const BINARY_NAMES: [&str; 2] = ["perllsp", "perl-dap"];
/// The exact native macOS target triples governed by issue #5432. Adoption is
/// restricted to these; no other triple may earn `adopt`.
pub(crate) const GOVERNED_TARGETS: [&str; 2] = ["aarch64-apple-darwin", "x86_64-apple-darwin"];

#[derive(Debug, Parser)]
#[command(name = "release-artifact-size")]
#[command(about = "Compare baseline and safe-ICF macOS release artifacts")]
struct Args {
    /// Rust target triple measured by this comparison.
    #[arg(long)]
    target: String,

    /// Directory containing extracted baseline perllsp and perl-dap binaries.
    #[arg(long)]
    baseline_dir: PathBuf,

    /// Directory containing extracted candidate perllsp and perl-dap binaries.
    #[arg(long)]
    candidate_dir: PathBuf,

    /// Baseline release-shaped .tar.gz archive.
    #[arg(long)]
    baseline_archive: PathBuf,

    /// Candidate release-shaped .tar.gz archive.
    #[arg(long)]
    candidate_archive: PathBuf,

    /// Baseline LSP smoke JSON receipt.
    #[arg(long)]
    baseline_lsp_smoke: PathBuf,

    /// Candidate LSP smoke JSON receipt.
    #[arg(long)]
    candidate_lsp_smoke: PathBuf,

    /// Baseline DAP smoke JSON receipt.
    #[arg(long)]
    baseline_dap_smoke: PathBuf,

    /// Candidate DAP smoke JSON receipt.
    #[arg(long)]
    candidate_dap_smoke: PathBuf,

    /// Full 40-character source SHA the baseline artifacts were built from.
    #[arg(long)]
    baseline_source_sha: String,

    /// Full 40-character source SHA the candidate artifacts were built from.
    #[arg(long)]
    candidate_source_sha: String,

    /// Declares that the confirming repeat measurement required by #5432 for a
    /// 0.5%-1.0% combined reduction has been performed.
    #[arg(long)]
    repeat_confirmed: bool,

    /// Minimum combined reduction, in basis points, required for adoption.
    #[arg(long, default_value_t = 50)]
    minimum_reduction_basis_points: i64,

    /// Minimum combined byte reduction required for adoption.
    #[arg(long, default_value_t = 131_072)]
    minimum_reduction_bytes: i64,

    /// Maximum permitted growth for either component, in basis points.
    #[arg(long, default_value_t = 25)]
    maximum_component_growth_basis_points: i64,

    /// Maximum permitted growth for either component, in bytes.
    #[arg(long, default_value_t = 32_768)]
    maximum_component_growth_bytes: i64,

    /// Combined reductions below this many basis points require one confirming
    /// repeat measurement before adoption.
    #[arg(long, default_value_t = 100)]
    repeat_required_below_basis_points: i64,

    /// Declared baseline linker flags. Must be empty for a valid safe-ICF A/B.
    #[arg(long, default_value = "")]
    baseline_rustflags: String,

    /// Declared candidate linker flags. Must equal the safe-ICF policy.
    #[arg(
        long,
        default_value = "-C linker=rust-lld -C linker-flavor=ld64.lld -C link-arg=--icf=safe"
    )]
    candidate_rustflags: String,

    /// JSON receipt output path.
    #[arg(long)]
    json: PathBuf,

    /// Optional Markdown summary output path.
    #[arg(long)]
    markdown: Option<PathBuf>,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let root = measure::project_root()?;
    check_distinct_outputs(&root, &args.json, args.markdown.as_deref())?;
    let receipt = evaluate(&root, &args)?;

    render::write_json(&root, &args.json, &receipt)?;
    if let Some(markdown) = &args.markdown {
        render::write_markdown(&root, markdown, &receipt)?;
    }

    println!(
        "safe-ICF comparison: target={} recommendation={} combined={} B ({} bp)",
        receipt.subject.target,
        receipt.recommendation.as_str(),
        receipt.comparison.combined.reduction_bytes,
        receipt.comparison.combined.reduction_basis_points
    );

    if receipt.recommendation.is_blocking() {
        bail!(
            "safe-ICF comparison did not establish an adopt/no-adopt decision; see {}",
            measure::display_path(&root, &measure::resolve_path(&root, &args.json))
        );
    }
    Ok(())
}

/// The Markdown summary is written after the JSON receipt, so identical
/// resolved paths would silently replace the machine-readable receipt with
/// prose. Fail before writing either output.
fn check_distinct_outputs(root: &Path, json: &Path, markdown: Option<&Path>) -> Result<()> {
    let Some(markdown) = markdown else {
        return Ok(());
    };
    let json_resolved = measure::resolve_path(root, json);
    let markdown_resolved = measure::resolve_path(root, markdown);
    if json_resolved == markdown_resolved {
        bail!(
            "--json and --markdown resolve to the same path `{}`; give each receipt its own output path",
            measure::display_path(root, &json_resolved)
        );
    }
    Ok(())
}

fn evaluate(root: &Path, args: &Args) -> Result<Receipt> {
    let policy = DecisionPolicy {
        minimum_reduction_basis_points: args.minimum_reduction_basis_points,
        minimum_reduction_bytes: args.minimum_reduction_bytes,
        maximum_component_growth_basis_points: args.maximum_component_growth_basis_points,
        maximum_component_growth_bytes: args.maximum_component_growth_bytes,
        repeat_required_below_basis_points: args.repeat_required_below_basis_points,
    };
    policy.validate()?;

    let mut limitations = Vec::new();
    let subject = measure::subject_identity(
        root,
        &args.target,
        &args.baseline_rustflags,
        &args.candidate_rustflags,
        &mut limitations,
    )?;
    let baseline = measure::measure_variant(
        root,
        &args.baseline_dir,
        &args.baseline_archive,
        &args.baseline_lsp_smoke,
        &args.baseline_dap_smoke,
        &args.baseline_source_sha,
        "baseline",
        &mut limitations,
    );
    let candidate = measure::measure_variant(
        root,
        &args.candidate_dir,
        &args.candidate_archive,
        &args.candidate_lsp_smoke,
        &args.candidate_dap_smoke,
        &args.candidate_source_sha,
        "candidate",
        &mut limitations,
    );

    let comparison = measure::compare_variants(
        &baseline,
        &candidate,
        &subject,
        &policy,
        &args.target,
        args.repeat_confirmed,
        &mut limitations,
    );
    let recommendation = measure::recommend(
        &subject,
        &baseline,
        &candidate,
        &comparison,
        &args.target,
        &args.baseline_rustflags,
        &args.candidate_rustflags,
        &mut limitations,
    );

    limitations.sort();
    limitations.dedup();

    Ok(Receipt {
        check: CHECK_NAME,
        schema_version: SCHEMA_VERSION,
        status: recommendation.status(),
        recommendation,
        claim_boundary: CLAIM_BOUNDARY,
        subject,
        policy,
        baseline,
        candidate,
        comparison,
        limitations,
    })
}

#[cfg(test)]
mod tests {
    use super::check_distinct_outputs;
    use std::path::{Path, PathBuf};

    #[test]
    fn identical_json_and_markdown_paths_are_rejected() {
        let root = Path::new("/repo");
        let error = check_distinct_outputs(root, Path::new("receipt"), Some(Path::new("receipt")))
            .expect_err("equal output paths must be rejected before writing");
        assert!(error.to_string().contains("same path"), "unexpected error: {error}");
    }

    #[test]
    fn equivalent_relative_and_absolute_output_paths_are_rejected() {
        let root = Path::new("/repo");
        assert!(
            check_distinct_outputs(
                root,
                Path::new("out/receipt.json"),
                Some(&PathBuf::from("/repo/out/receipt.json")),
            )
            .is_err()
        );
    }

    #[test]
    fn distinct_or_absent_markdown_paths_are_accepted() {
        let root = Path::new("/repo");
        assert!(
            check_distinct_outputs(
                root,
                Path::new("out/receipt.json"),
                Some(Path::new("out/receipt.md")),
            )
            .is_ok()
        );
        assert!(check_distinct_outputs(root, Path::new("out/receipt.json"), None).is_ok());
    }
}
