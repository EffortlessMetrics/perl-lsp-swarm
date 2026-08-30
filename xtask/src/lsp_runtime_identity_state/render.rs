use std::collections::BTreeMap;
use std::fmt::Write as _;

use color_eyre::eyre::{Result, bail, eyre};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use super::*;

pub(super) fn semantic_digest(vocabulary: &Vocabulary) -> Result<String> {
    let value = serde_json::to_value(vocabulary)?;
    let mut canonical = String::new();
    write_canonical_json(&value, &mut canonical)?;
    let digest = Sha256::digest(canonical.as_bytes());
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        write!(&mut encoded, "{byte:02x}")
            .map_err(|_| eyre!("failed to encode semantic digest"))?;
    }
    Ok(encoded)
}

fn write_canonical_json(value: &Value, output: &mut String) -> Result<()> {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) => output.push_str(&value.to_string()),
        Value::String(value) => output.push_str(&serde_json::to_string(value)?),
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_canonical_json(value, output)?;
            }
            output.push(']');
        }
        Value::Object(values) => {
            output.push('{');
            let sorted: BTreeMap<&str, &Value> =
                values.iter().map(|(key, value)| (key.as_str(), value)).collect();
            for (index, (key, value)) in sorted.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                output.push_str(&serde_json::to_string(key)?);
                output.push(':');
                write_canonical_json(value, output)?;
            }
            output.push('}');
        }
    }
    Ok(())
}

pub(super) fn render_index(vocabulary: &Vocabulary) -> Result<String> {
    let mut output = String::new();
    writeln!(&mut output, "<!-- BEGIN CHECKED LSP RUNTIME VOCABULARY INDEX -->")?;
    writeln!(&mut output, "## Checked machine index")?;
    writeln!(&mut output)?;
    writeln!(&mut output, "- **Schema:** `{}` v{}", vocabulary.schema, vocabulary.version)?;
    writeln!(
        &mut output,
        "- **Authority:** #{} under #{}; checked train #{}",
        vocabulary.authority.issue,
        vocabulary.authority.architecture,
        vocabulary.authority.train
    )?;
    write_ids(&mut output, "Axes", vocabulary.axes.iter().map(|row| row.id.as_str()))?;
    write_ids(
        &mut output,
        "Identities",
        vocabulary.identities.iter().map(|row| row.id.as_str()),
    )?;
    write_ids(
        &mut output,
        "Boundary terms",
        vocabulary.boundary_terms.iter().map(|row| row.id.as_str()),
    )?;
    write_ids(
        &mut output,
        "States",
        vocabulary.states.iter().map(|row| row.id.as_str()),
    )?;
    write_ids(
        &mut output,
        "Relationships",
        vocabulary.relations.iter().map(|row| row.id.as_str()),
    )?;
    write_ids(
        &mut output,
        "Ambiguous terms",
        vocabulary.ambiguous_terms.iter().map(|row| row.term.as_str()),
    )?;
    write_ids(
        &mut output,
        "Journeys",
        vocabulary.journeys.iter().map(|row| row.id.as_str()),
    )?;
    writeln!(&mut output, "<!-- END CHECKED LSP RUNTIME VOCABULARY INDEX -->")?;
    Ok(output)
}

fn write_ids<'a>(
    output: &mut String,
    label: &str,
    values: impl Iterator<Item = &'a str>,
) -> std::fmt::Result {
    let joined = values.map(|value| format!("`{value}`")).collect::<Vec<_>>().join(", ");
    writeln!(output, "- **{label}:** {joined}")
}

pub(super) fn verify_document(vocabulary: &Vocabulary, document: &str) -> Result<()> {
    for required in [
        vocabulary.authority.claim.as_str(),
        vocabulary.request_state.law.as_str(),
        vocabulary.generic_boundary.one_authority.law.as_str(),
        vocabulary.generic_boundary.currentness_law.as_str(),
    ] {
        if !document.contains(required) {
            bail!("human reference omits normative law {required:?}");
        }
    }

    let begin = "<!-- BEGIN CHECKED LSP RUNTIME VOCABULARY INDEX -->";
    let end = "<!-- END CHECKED LSP RUNTIME VOCABULARY INDEX -->";
    let start = document
        .find(begin)
        .ok_or_else(|| eyre!("human reference lacks checked-index start marker"))?;
    let finish = document[start..]
        .find(end)
        .map(|offset| start + offset + end.len())
        .ok_or_else(|| eyre!("human reference lacks checked-index end marker"))?;
    let actual = document[start..finish].trim_end();
    let expected = render_index(vocabulary)?;
    if actual != expected.trim_end() {
        bail!("checked machine index is stale; regenerate it from the machine source");
    }
    Ok(())
}
