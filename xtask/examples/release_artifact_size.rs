//! Compare same-SHA baseline and safe-ICF macOS release artifacts.
//!
//! This is a measurement instrument, not a build or publication authority. It
//! consumes already-built, already-stripped, release-shaped artifacts and emits
//! a deterministic decision receipt for issue #5432.

#![allow(clippy::print_stderr, clippy::print_stdout)]

use clap::Parser;
use color_eyre::eyre::{Result, bail};
use std::path::{Component, Path, PathBuf};

#[path = "../src/bin/release_artifact_size/measure.rs"]
mod measure;
#[path = "../src/bin/release_artifact_size/model.rs"]
mod model;
#[path = "../src/bin/release_artifact_size/policy.rs"]
mod policy;
#[path = "../src/bin/release_artifact_size/render.rs"]
mod render;

use model::{DecisionPolicy, Receipt};

// Re-exported so `measure` keeps resolving them through `super`, and so the
// shadow-lane contract proof can bind the workflow to the same constants
// instead of restating them.
pub(crate) use policy::{BINARY_NAMES, GOVERNED_TARGETS, REPOSITORY, SAFE_ICF_RUSTFLAGS};

pub(crate) const CHECK_NAME: &str = "release-artifact-size";
pub(crate) const SCHEMA_VERSION: &str = "release_artifact_size.v1";
pub(crate) const CLAIM_BOUNDARY: &str = concat!(
    "same-SHA post-strip size and packaged-binary smoke comparison for perllsp ",
    "and perl-dap on one native macOS target; does not prove startup/RSS ",
    "improvement, other targets, publication, or general release readiness"
);

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
    #[arg(long, default_value = SAFE_ICF_RUSTFLAGS)]
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
    check_output_paths(&root, &args)?;
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

/// Resolve against the repository root and then remove `.` and `..`
/// components, so `out/receipt.json` and `out/sub/../receipt.json` compare
/// equal. This is lexical on purpose: outputs usually do not exist yet, so
/// `canonicalize` would fail on exactly the paths that need checking.
fn normalized_output_path(root: &Path, path: &Path) -> PathBuf {
    let resolved = measure::resolve_path(root, path);
    let mut normalized = PathBuf::new();
    for component in resolved.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// Guard both receipt outputs before any evidence is read or written.
///
/// The Markdown summary is written after the JSON receipt, so equal paths
/// would silently replace the machine-readable receipt with prose. An output
/// aliasing a measured input would destroy the evidence a rerun needs.
fn check_output_paths(root: &Path, args: &Args) -> Result<()> {
    let json = normalized_output_path(root, &args.json);
    let markdown = args.markdown.as_deref().map(|path| normalized_output_path(root, path));

    if markdown.as_ref() == Some(&json) {
        bail!(
            "--json and --markdown resolve to the same path `{}`; give each receipt its own output path",
            measure::display_path(root, &json)
        );
    }

    let mut inputs = vec![
        args.baseline_archive.clone(),
        args.candidate_archive.clone(),
        args.baseline_lsp_smoke.clone(),
        args.candidate_lsp_smoke.clone(),
        args.baseline_dap_smoke.clone(),
        args.candidate_dap_smoke.clone(),
    ];
    for directory in [&args.baseline_dir, &args.candidate_dir] {
        inputs.push(directory.clone());
        for name in BINARY_NAMES {
            inputs.push(directory.join(name));
        }
    }

    for input in &inputs {
        let input = normalized_output_path(root, input);
        for (flag, output) in [("--json", Some(&json)), ("--markdown", markdown.as_ref())] {
            if output == Some(&input) {
                bail!(
                    "{flag} output `{}` is also a measured input; writing it would destroy the evidence this comparison consumed",
                    measure::display_path(root, &input)
                );
            }
        }
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
    use super::{Args, SAFE_ICF_RUSTFLAGS, check_output_paths};
    use std::path::{Path, PathBuf};

    const ROOT: &str = "/repo";
    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    fn args(json: &str, markdown: Option<&str>) -> Args {
        Args {
            target: "aarch64-apple-darwin".to_string(),
            baseline_dir: PathBuf::from("evidence/baseline"),
            candidate_dir: PathBuf::from("evidence/candidate"),
            baseline_archive: PathBuf::from("evidence/baseline.tar.gz"),
            candidate_archive: PathBuf::from("evidence/candidate.tar.gz"),
            baseline_lsp_smoke: PathBuf::from("evidence/baseline-lsp.json"),
            candidate_lsp_smoke: PathBuf::from("evidence/candidate-lsp.json"),
            baseline_dap_smoke: PathBuf::from("evidence/baseline-dap.json"),
            candidate_dap_smoke: PathBuf::from("evidence/candidate-dap.json"),
            baseline_source_sha: SHA.to_string(),
            candidate_source_sha: SHA.to_string(),
            repeat_confirmed: false,
            minimum_reduction_basis_points: 50,
            minimum_reduction_bytes: 131_072,
            maximum_component_growth_basis_points: 25,
            maximum_component_growth_bytes: 32_768,
            repeat_required_below_basis_points: 100,
            baseline_rustflags: String::new(),
            candidate_rustflags: SAFE_ICF_RUSTFLAGS.to_string(),
            json: PathBuf::from(json),
            markdown: markdown.map(PathBuf::from),
        }
    }

    #[test]
    fn identical_json_and_markdown_paths_are_rejected() {
        let error = check_output_paths(Path::new(ROOT), &args("receipt", Some("receipt")))
            .expect_err("equal output paths must be rejected before writing");
        assert!(error.to_string().contains("same path"), "unexpected error: {error}");
    }

    #[test]
    fn equivalent_relative_and_absolute_output_paths_are_rejected() {
        assert!(
            check_output_paths(
                Path::new(ROOT),
                &args("out/receipt.json", Some("/repo/out/receipt.json")),
            )
            .is_err()
        );
    }

    #[test]
    fn output_paths_aliased_through_dot_segments_are_rejected() {
        assert!(
            check_output_paths(
                Path::new(ROOT),
                &args("out/receipt.json", Some("out/sub/../receipt.json")),
            )
            .is_err()
        );
        assert!(
            check_output_paths(
                Path::new(ROOT),
                &args("./out/receipt.json", Some("out/receipt.md"))
            )
            .is_ok()
        );
    }

    #[test]
    fn an_output_that_aliases_a_measured_input_is_rejected() {
        for aliased in [
            "evidence/baseline.tar.gz",
            "evidence/candidate-dap.json",
            "evidence/baseline/perllsp",
            "evidence/candidate/perl-dap",
            "evidence/baseline/../baseline/perllsp",
        ] {
            assert!(
                check_output_paths(Path::new(ROOT), &args(aliased, None)).is_err(),
                "`{aliased}` must be rejected as an output path"
            );
        }
    }

    #[test]
    fn distinct_or_absent_markdown_paths_are_accepted() {
        assert!(
            check_output_paths(Path::new(ROOT), &args("out/receipt.json", Some("out/receipt.md")))
                .is_ok()
        );
        assert!(check_output_paths(Path::new(ROOT), &args("out/receipt.json", None)).is_ok());
    }
}
