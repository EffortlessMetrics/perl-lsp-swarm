const CLASSIFICATION_OUTPUT_SCHEMA_VERSION: &str =
    "perl_core_harness.transition_classification.v1";

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ClassificationOutput {
    schema_version: String,
    transition: CompatibilityTransition,
    reason: String,
    requires_candidate: bool,
    semantic_boundary_change: bool,
    accepted_state_change_permitted: bool,
    claim_boundary: String,
}

fn classify_command(config: ClassifyConfig) -> Result<()> {
    reject_output_aliases(
        &[config.accepted_baseline.clone(), config.compile.clone()],
        &config.output,
    )?;
    let classification = classify_paths(&config.accepted_baseline, &config.compile)?;
    write_json(&config.output, &classification)
}

fn classify_paths(accepted_path: &Path, compile_path: &Path) -> Result<ClassificationOutput> {
    let accepted = read_accepted_baseline_v2(accepted_path)?;
    validate_accepted_baseline_shape(&accepted)?;
    let compile: RunReport = read_json(compile_path, "compile observation")?;
    validate_compile_report_shape(&compile)?;
    Ok(classification_output(classify_transition(
        &AcceptedBaseline::V2(Box::new(accepted)),
        &compile,
    )))
}

fn classification_output(classification: Classification) -> ClassificationOutput {
    ClassificationOutput {
        schema_version: CLASSIFICATION_OUTPUT_SCHEMA_VERSION.to_string(),
        transition: classification.transition,
        reason: classification.reason,
        requires_candidate: classification.requires_candidate,
        semantic_boundary_change: classification.semantic_boundary_change,
        accepted_state_change_permitted: false,
        claim_boundary: "classifies one complete compile observation against a pre-existing accepted V2 ratchet; it cannot accept or lower that ratchet".into(),
    }
}

fn read_accepted_baseline_v2(path: &Path) -> Result<CompileBaselineV2> {
    let bytes =
        fs::read(path).with_context(|| format!("reading accepted baseline {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("decoding accepted baseline {}", path.display()))?;
    let schema = value
        .get("schema_version")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("missing");
    if schema != COMPILE_BASELINE_V2_SCHEMA_VERSION {
        bail!(
            "this CLI slice accepts only {COMPILE_BASELINE_V2_SCHEMA_VERSION}; found {schema}"
        );
    }
    serde_json::from_value(value).context("decoding accepted compile baseline v2")
}

fn validate_accepted_baseline_shape(accepted: &CompileBaselineV2) -> Result<()> {
    let passed = accepted
        .file_results
        .iter()
        .filter(|result| result.status == RunnerStatus::Pass)
        .count();
    let failed = accepted.file_results.len().saturating_sub(passed);
    if accepted.files_total != accepted.file_results.len()
        || accepted.files_passed != passed
        || accepted.files_failed != failed
        || accepted.files_passed.saturating_add(accepted.files_failed) != accepted.files_total
    {
        bail!("accepted ratchet file counts are internally inconsistent with file_results");
    }
    Ok(())
}

fn validate_compile_report_shape(report: &RunReport) -> Result<()> {
    if report.schema_version != RUN_REPORT_SCHEMA_VERSION {
        bail!(
            "compile observation schema_version must be {RUN_REPORT_SCHEMA_VERSION}, found {}",
            report.schema_version
        );
    }
    if report.harness_status != Some(0) {
        bail!(
            "compile observation harness_status must be Some(0) for a complete successful run; found {:?}",
            report.harness_status
        );
    }
    let passed = report
        .file_results
        .iter()
        .filter(|result| result.status == RunnerStatus::Pass)
        .count();
    let failed = report.file_results.len().saturating_sub(passed);
    if report.summary.files_total != report.file_results.len()
        || report.summary.files_passed != passed
        || report.summary.files_failed != failed
        || report
            .summary
            .files_passed
            .saturating_add(report.summary.files_failed)
            != report.summary.files_total
    {
        bail!("compile observation file counts are internally inconsistent with file_results");
    }
    for result in &report.file_results {
        if result.assertions_passed > result.assertions_total {
            bail!(
                "compile observation file {} passes more assertions than it declares",
                result.path
            );
        }
    }
    Ok(())
}

fn read_json<T: DeserializeOwned>(path: &Path, label: &str) -> Result<T> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading {label} {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("decoding {label} {}", path.display()))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating output directory {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(value).context("serializing classification output")?;
    fs::write(path, format!("{json}\n"))
        .with_context(|| format!("writing classification output {}", path.display()))
}

fn reject_output_aliases(inputs: &[PathBuf], output: &Path) -> Result<()> {
    let output = resolve_destination(output)?;
    for input in inputs {
        let input = fs::canonicalize(input)
            .with_context(|| format!("canonicalizing input {}", input.display()))?;
        if input == output {
            bail!("classification output aliases an input evidence file");
        }
        // Unix hard links keep distinct pathnames after canonicalize; compare
        // device/inode identity so classify cannot truncate evidence through an
        // alias. Windows hard-link identity needs unstable `windows_by_handle`
        // and remains a follow-up for this lean slice.
        if same_unix_file(&input, &output)? {
            bail!("classification output aliases an input evidence file");
        }
    }
    Ok(())
}

#[cfg(unix)]
fn same_unix_file(left: &Path, right: &Path) -> Result<bool> {
    use std::os::unix::fs::MetadataExt;
    if !left.exists() || !right.exists() {
        return Ok(false);
    }
    let left_meta =
        fs::metadata(left).with_context(|| format!("reading metadata for {}", left.display()))?;
    let right_meta =
        fs::metadata(right).with_context(|| format!("reading metadata for {}", right.display()))?;
    Ok(left_meta.dev() == right_meta.dev() && left_meta.ino() == right_meta.ino())
}

#[cfg(not(unix))]
fn same_unix_file(_left: &Path, _right: &Path) -> Result<bool> {
    Ok(false)
}

fn resolve_destination(path: &Path) -> Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            bail!("classification output must not be a symlink path")
        }
        Ok(_) => fs::canonicalize(path)
            .with_context(|| format!("canonicalizing output {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let absolute = if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()
                    .context("reading current directory")?
                    .join(path)
            };
            let parent = absolute
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            let file_name = absolute.file_name().ok_or_else(|| {
                color_eyre::eyre::eyre!("classification output path has no file name")
            })?;
            fs::create_dir_all(parent)
                .with_context(|| format!("creating output directory {}", parent.display()))?;
            let parent = fs::canonicalize(parent)
                .with_context(|| format!("canonicalizing output parent {}", parent.display()))?;
            Ok(parent.join(file_name))
        }
        Err(error) => Err(error).with_context(|| {
            format!("reading classification output metadata {}", path.display())
        }),
    }
}
