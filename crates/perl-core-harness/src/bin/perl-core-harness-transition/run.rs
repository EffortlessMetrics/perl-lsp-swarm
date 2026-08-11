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
    reject_output_aliases(&[config.accepted_baseline.clone(), config.compile.clone()], &config.output)?;
    let classification = classify_paths(&config.accepted_baseline, &config.compile)?;
    write_json(&config.output, &classification)
}

fn classify_paths(accepted_path: &Path, compile_path: &Path) -> Result<ClassificationOutput> {
    let accepted = read_accepted_baseline_v2(accepted_path)?;
    let compile: RunReport = read_json(compile_path, "compile observation")?;
    if compile.schema_version != RUN_REPORT_SCHEMA_VERSION {
        bail!(
            "compile observation schema_version must be {RUN_REPORT_SCHEMA_VERSION}, found {}",
            compile.schema_version
        );
    }
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
    }
    Ok(())
}

fn resolve_destination(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return fs::canonicalize(path)
            .with_context(|| format!("canonicalizing output {}", path.display()));
    }
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
    let file_name = absolute
        .file_name()
        .ok_or_else(|| color_eyre::eyre::eyre!("classification output path has no file name"))?;
    let parent = fs::canonicalize(parent)
        .with_context(|| format!("canonicalizing output parent {}", parent.display()))?;
    Ok(parent.join(file_name))
}
