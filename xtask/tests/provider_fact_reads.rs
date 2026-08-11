use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Component, Path, PathBuf};

const INVENTORY_PATH: &str = "policy/provider-fact-reads.toml";
const EXPECTED_POLICY: &str = "provider-fact-reads";
const EXPECTED_OWNER_ISSUE: u64 = 6815;
const EXPECTED_SCHEMA_VERSION: u32 = 1;
const REQUIRED_PROVIDERS: &[&str] = &[
    "completion",
    "definition",
    "references",
    "hover",
    "diagnostics",
    "rename",
    "safe_delete",
    "workspace_symbols",
    "document_symbols",
    "semantic_tokens",
];
const ALLOWED_PRODUCERS: &[&str] = &[
    "current_document",
    "workspace_index",
    "semantic_queries",
    "semantic_shadow",
    "runtime_mixed",
];
const ALLOWED_PROOF_CLASSES: &[&str] = &["mixed", "shadow", "edit_authorizing"];
const ALLOWED_DISPOSITIONS: &[&str] = &[
    "port_candidate",
    "intentional_provider_policy",
    "retire_after_parity",
];

#[derive(Debug, Deserialize)]
struct Inventory {
    schema_version: u32,
    policy: String,
    owner_issue: u64,
    generated_status: String,
    required_providers: Vec<String>,
    allowed_producers: Vec<String>,
    allowed_proof_classes: Vec<String>,
    allowed_dispositions: Vec<String>,
    #[serde(rename = "read")]
    reads: Vec<FactRead>,
}

#[derive(Debug, Deserialize)]
struct FactRead {
    id: String,
    provider: String,
    request_class: String,
    query: String,
    source_path: String,
    source_anchors: Vec<String>,
    producer: String,
    proof_class: String,
    readiness_input: String,
    fallback: String,
    duplicate_interpretation: String,
    migration_disposition: String,
    replacement_owner: String,
}

#[test]
fn provider_fact_read_inventory_is_valid_and_fresh() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask manifest directory must have a repository parent")?;
    let inventory_text = fs::read_to_string(root.join(INVENTORY_PATH))
        .with_context(|| format!("failed to read {INVENTORY_PATH}"))?;
    let inventory: Inventory =
        toml::from_str(&inventory_text).context("failed to parse provider fact-read inventory")?;

    validate_inventory(&root, &inventory)?;

    let expected_status = render_status(&inventory)?;
    let actual_status = fs::read_to_string(root.join(&inventory.generated_status))
        .with_context(|| format!("failed to read {}", inventory.generated_status))?;
    if actual_status != expected_status {
        bail!(
            "{} is stale relative to {INVENTORY_PATH}",
            inventory.generated_status
        );
    }

    Ok(())
}

fn validate_inventory(root: &Path, inventory: &Inventory) -> Result<()> {
    if inventory.schema_version != EXPECTED_SCHEMA_VERSION {
        bail!(
            "schema_version must be {EXPECTED_SCHEMA_VERSION}, found {}",
            inventory.schema_version
        );
    }
    if inventory.policy != EXPECTED_POLICY {
        bail!(
            "policy must be {EXPECTED_POLICY:?}, found {:?}",
            inventory.policy
        );
    }
    if inventory.owner_issue != EXPECTED_OWNER_ISSUE {
        bail!(
            "owner_issue must be {EXPECTED_OWNER_ISSUE}, found {}",
            inventory.owner_issue
        );
    }
    require_exact_vocabulary(
        "required_providers",
        &inventory.required_providers,
        REQUIRED_PROVIDERS,
    )?;
    require_exact_vocabulary(
        "allowed_producers",
        &inventory.allowed_producers,
        ALLOWED_PRODUCERS,
    )?;
    require_exact_vocabulary(
        "allowed_proof_classes",
        &inventory.allowed_proof_classes,
        ALLOWED_PROOF_CLASSES,
    )?;
    require_exact_vocabulary(
        "allowed_dispositions",
        &inventory.allowed_dispositions,
        ALLOWED_DISPOSITIONS,
    )?;

    if inventory.reads.is_empty() {
        bail!("inventory must contain at least one [[read]] row");
    }

    let required_provider_set = string_set(REQUIRED_PROVIDERS);
    let producer_set = string_set(ALLOWED_PRODUCERS);
    let proof_class_set = string_set(ALLOWED_PROOF_CLASSES);
    let disposition_set = string_set(ALLOWED_DISPOSITIONS);
    let mut seen_ids = BTreeSet::new();
    let mut provider_counts = BTreeMap::<String, usize>::new();

    for read in &inventory.reads {
        require_non_empty("id", &read.id)?;
        require_non_empty("provider", &read.provider)?;
        require_non_empty("request_class", &read.request_class)?;
        require_non_empty("query", &read.query)?;
        require_non_empty("source_path", &read.source_path)?;
        require_non_empty("producer", &read.producer)?;
        require_non_empty("proof_class", &read.proof_class)?;
        require_non_empty("readiness_input", &read.readiness_input)?;
        require_non_empty("fallback", &read.fallback)?;
        require_non_empty(
            "duplicate_interpretation",
            &read.duplicate_interpretation,
        )?;
        require_non_empty("migration_disposition", &read.migration_disposition)?;
        require_non_empty("replacement_owner", &read.replacement_owner)?;

        if !seen_ids.insert(read.id.clone()) {
            bail!("duplicate fact-read id {:?}", read.id);
        }
        let provider_prefix = format!("{}.", read.provider);
        if !read.id.starts_with(provider_prefix.as_str()) {
            bail!(
                "fact-read id {:?} must start with provider prefix {:?}",
                read.id,
                provider_prefix
            );
        }
        if !required_provider_set.contains(&read.provider) {
            bail!(
                "fact-read {:?} uses ungoverned provider {:?}",
                read.id,
                read.provider
            );
        }
        if !producer_set.contains(&read.producer) {
            bail!(
                "fact-read {:?} uses unsupported producer {:?}",
                read.id,
                read.producer
            );
        }
        if !proof_class_set.contains(&read.proof_class) {
            bail!(
                "fact-read {:?} uses unsupported proof class {:?}",
                read.id,
                read.proof_class
            );
        }
        if !disposition_set.contains(&read.migration_disposition) {
            bail!(
                "fact-read {:?} uses unsupported migration disposition {:?}",
                read.id,
                read.migration_disposition
            );
        }
        validate_replacement_owner(&read.id, &read.replacement_owner)?;

        let relative_source = Path::new(&read.source_path);
        if relative_source.is_absolute()
            || relative_source
                .components()
                .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
        {
            bail!(
                "fact-read {:?} source_path must be repository-relative: {:?}",
                read.id,
                read.source_path
            );
        }
        if read.source_anchors.is_empty() {
            bail!(
                "fact-read {:?} must name at least one source anchor",
                read.id
            );
        }

        let source = fs::read_to_string(root.join(relative_source)).with_context(|| {
            format!(
                "fact-read {:?} failed to read source path {}",
                read.id, read.source_path
            )
        })?;
        let mut seen_anchors = BTreeSet::new();
        for anchor in &read.source_anchors {
            require_non_empty("source_anchor", anchor)?;
            if !seen_anchors.insert(anchor) {
                bail!(
                    "fact-read {:?} repeats source anchor {:?}",
                    read.id,
                    anchor
                );
            }
            if !source.contains(anchor) {
                bail!(
                    "fact-read {:?} source anchor {:?} is absent from {}",
                    read.id,
                    anchor,
                    read.source_path
                );
            }
        }

        *provider_counts.entry(read.provider.clone()).or_default() += 1;
    }

    for provider in REQUIRED_PROVIDERS {
        if provider_counts
            .get(*provider)
            .copied()
            .unwrap_or_default()
            == 0
        {
            bail!("required provider {provider:?} has no inventoried fact read");
        }
    }

    Ok(())
}

fn require_exact_vocabulary(label: &str, actual: &[String], expected: &[&str]) -> Result<()> {
    let actual_set: BTreeSet<String> = actual.iter().cloned().collect();
    let expected_set = string_set(expected);
    if actual_set != expected_set {
        bail!(
            "{label} must equal {:?}, found {:?}",
            expected_set,
            actual_set
        );
    }
    if actual.len() != actual_set.len() {
        bail!("{label} contains duplicate values");
    }
    Ok(())
}

fn string_set(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn require_non_empty(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{label} must not be empty");
    }
    Ok(())
}

fn validate_replacement_owner(id: &str, owners: &str) -> Result<()> {
    for owner in owners.split('/') {
        let number = owner
            .strip_prefix('#')
            .context("replacement owner must start with '#'")?;
        if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            bail!("fact-read {id:?} has invalid replacement owner token {owner:?}");
        }
    }
    Ok(())
}

fn render_status(inventory: &Inventory) -> Result<String> {
    let mut output = String::new();
    writeln!(output, "# Provider Fact Read Inventory")?;
    writeln!(output)?;
    writeln!(
        output,
        "> Generated from `{INVENTORY_PATH}`. This inventory records"
    )?;
    writeln!(
        output,
        "> current provider fact reads, ownership assumptions, and duplicate interpretation"
    )?;
    writeln!(
        output,
        "> seams. It does not change provider behavior, promote a producer, or authorize edits."
    )?;
    writeln!(output)?;
    writeln!(
        output,
        "Owner: [#{}](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/{})",
        inventory.owner_issue, inventory.owner_issue
    )?;
    writeln!(output)?;
    writeln!(output, "## Coverage")?;
    writeln!(output)?;
    writeln!(output, "| Provider | Inventoried reads |")?;
    writeln!(output, "| --- | ---: |")?;

    let mut counts = BTreeMap::<&str, usize>::new();
    for read in &inventory.reads {
        *counts.entry(read.provider.as_str()).or_default() += 1;
    }
    for provider in &inventory.required_providers {
        writeln!(
            output,
            "| `{}` | {} |",
            escape_cell(provider),
            counts
                .get(provider.as_str())
                .copied()
                .unwrap_or_default()
        )?;
    }

    writeln!(output)?;
    writeln!(output, "## Reads")?;
    writeln!(output)?;
    writeln!(
        output,
        "| ID | Provider | Request | Query / fact need | Current producer | Proof assumption | Readiness / freshness | Fallback / refusal | Duplicate interpretation seam | Migration |"
    )?;
    writeln!(
        output,
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |"
    )?;
    for read in &inventory.reads {
        writeln!(
            output,
            "| `{}` | `{}` | `{}` | {} | `{}` | `{}` | {} | {} | {} | `{}` → {} |",
            escape_cell(&read.id),
            escape_cell(&read.provider),
            escape_cell(&read.request_class),
            escape_cell(&read.query),
            escape_cell(&read.producer),
            escape_cell(&read.proof_class),
            escape_cell(&read.readiness_input),
            escape_cell(&read.fallback),
            escape_cell(&read.duplicate_interpretation),
            escape_cell(&read.migration_disposition),
            escape_cell(&read.replacement_owner),
        )?;
    }

    writeln!(output)?;
    writeln!(output, "## Claim boundary")?;
    writeln!(output)?;
    writeln!(output, "- A producer name is not a proof or safety class.")?;
    writeln!(output, "- An inventory row is not a cutover decision.")?;
    writeln!(
        output,
        "- `port_candidate` means the read should move behind the canonical provider port."
    )?;
    writeln!(
        output,
        "- `intentional_provider_policy` means domain policy may remain provider-owned after shared facts arrive."
    )?;
    writeln!(
        output,
        "- `retire_after_parity` requires request-bound comparison evidence before removal."
    )?;
    writeln!(
        output,
        "- Generated, dynamic, stale, partial, ambiguous, or low-confidence facts do not gain edit authority from this inventory."
    )?;

    Ok(output)
}

fn escape_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', "<br>")
}
