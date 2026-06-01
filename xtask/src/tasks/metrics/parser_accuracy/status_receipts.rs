use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Context, Result, eyre};
use serde::Serialize;

use super::{
    Cadence, DEFAULT_MANIFEST, DEFAULT_OUTPUT, FAILURE_PACKET_STATUS_RECEIPT,
    FAILURE_WORKLIST_STATUS_RECEIPT, FIXTURE_INVENTORY_STATUS_RECEIPT, FailurePacket,
    FixtureMetadata, LabelMode, MetricRow, NEXT_POINTER_STATUS_RECEIPT, ParserAccuracyArtifact,
    ParserAccuracyManifest, read_manifest, stable_hash, validate_artifact_contract,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusReceiptFile {
    pub name: &'static str,
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug, Serialize)]
struct FailurePacketStatusReceipt {
    schema_version: u32,
    commit: String,
    cadence: Cadence,
    generated_by: &'static str,
    failure_packet_count: u64,
    failure_packets: Vec<FailurePacketStatusEntry>,
}

#[derive(Debug, Serialize)]
struct FailurePacketStatusEntry {
    id: String,
    failure_kind: String,
    likely_layer: String,
    fixture_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metric: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u64>,
    expected: Vec<String>,
    actual: Vec<String>,
    actual_nearest: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_excerpt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    suggested_next_fix: Option<String>,
    suggested_next_pr: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    first_observed_commit: Option<String>,
}

#[derive(Debug, Serialize)]
struct FixtureInventoryStatusReceipt {
    schema_version: u32,
    commit: String,
    generated_by: &'static str,
    fixture_count: u64,
    family_count: u64,
    scored_lines: u64,
    scored_symbols: u64,
    fixtures: Vec<FixtureInventoryStatusEntry>,
}

#[derive(Debug, Serialize)]
struct FixtureInventoryStatusEntry {
    id: String,
    family: String,
    source_path: String,
    label_mode: LabelMode,
    scored_lines: u64,
    scored_symbols: u64,
    fully_labeled_regions: u64,
    partial_labeled_regions: u64,
    unknown_regions: u64,
    negative_regions: u64,
    dynamic_boundaries: u64,
    unsupported_constructs: u64,
    real_project_file: bool,
    generated: bool,
    provider_expectation_counts: ProviderExpectationCounts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FailureWorklistRow {
    family: String,
    count: u64,
    likely_layer: String,
    first_fixture: String,
    suggested_pr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MeasurementGapRow {
    metric: String,
    reason: String,
    suggested_pr: String,
}

#[derive(Debug, Serialize)]
struct ProviderExpectationCounts {
    method_completion: u64,
    diagnostics: u64,
    navigation: u64,
}

pub fn status_receipt_files_from_target(root: &Path) -> Result<Vec<StatusReceiptFile>> {
    let manifest_path = root.join(DEFAULT_MANIFEST);
    let manifest = read_manifest(root, &manifest_path)?;
    let artifact_path = root.join(DEFAULT_OUTPUT);
    let raw = fs::read_to_string(&artifact_path)
        .with_context(|| format!("reading parser accuracy artifact {}", artifact_path.display()))?;
    let artifact: ParserAccuracyArtifact = serde_json::from_str(&raw)
        .with_context(|| format!("parsing parser accuracy artifact {}", artifact_path.display()))?;
    validate_artifact_contract(&artifact)?;
    status_receipt_files(root, &manifest, &artifact)
}

pub fn status_receipt_equivalent_ignoring_commit(existing: &str, generated: &str) -> bool {
    let Ok(mut existing_value) = serde_json::from_str::<serde_json::Value>(existing) else {
        return existing == generated;
    };
    let Ok(mut generated_value) = serde_json::from_str::<serde_json::Value>(generated) else {
        return existing == generated;
    };
    if let (Some(existing_object), Some(generated_object)) =
        (existing_value.as_object_mut(), generated_value.as_object_mut())
    {
        existing_object.remove("commit");
        generated_object.remove("commit");
    }
    existing_value == generated_value
}

pub(super) fn write_status_receipts(
    root: &Path,
    manifest: &ParserAccuracyManifest,
    artifact: &ParserAccuracyArtifact,
) -> Result<()> {
    for receipt in status_receipt_files(root, manifest, artifact)? {
        let parent = receipt
            .path
            .parent()
            .ok_or_else(|| eyre!("parser accuracy status receipt path has no parent"))?;
        fs::create_dir_all(parent).with_context(|| {
            format!("creating parser accuracy status receipt dir {}", parent.display())
        })?;
        fs::write(&receipt.path, receipt.content)
            .with_context(|| format!("writing {}", receipt.path.display()))?;
        println!("parser accuracy status receipt written: {}", receipt.path.display());
    }
    Ok(())
}

pub(super) fn status_receipt_files(
    root: &Path,
    manifest: &ParserAccuracyManifest,
    artifact: &ParserAccuracyArtifact,
) -> Result<Vec<StatusReceiptFile>> {
    Ok(vec![
        StatusReceiptFile {
            name: FAILURE_PACKET_STATUS_RECEIPT,
            path: root.join(FAILURE_PACKET_STATUS_RECEIPT),
            content: render_failure_packet_status_receipt(artifact)?,
        },
        StatusReceiptFile {
            name: FIXTURE_INVENTORY_STATUS_RECEIPT,
            path: root.join(FIXTURE_INVENTORY_STATUS_RECEIPT),
            content: render_fixture_inventory_status_receipt(manifest, artifact)?,
        },
        StatusReceiptFile {
            name: FAILURE_WORKLIST_STATUS_RECEIPT,
            path: root.join(FAILURE_WORKLIST_STATUS_RECEIPT),
            content: render_failure_worklist_status_receipt(artifact),
        },
        StatusReceiptFile {
            name: NEXT_POINTER_STATUS_RECEIPT,
            path: root.join(NEXT_POINTER_STATUS_RECEIPT),
            content: render_next_pointer_status_receipt(artifact),
        },
    ])
}

pub(super) fn render_failure_packet_status_receipt(
    artifact: &ParserAccuracyArtifact,
) -> Result<String> {
    let receipt = FailurePacketStatusReceipt {
        schema_version: 1,
        commit: artifact.commit.clone(),
        cadence: artifact.cadence,
        generated_by: "cargo xtask metrics parser-accuracy --export-status-receipts",
        failure_packet_count: artifact.failure_packets.len() as u64,
        failure_packets: artifact.failure_packets.iter().map(failure_packet_status_entry).collect(),
    };
    render_json_with_newline(&receipt)
}

fn failure_packet_status_entry(packet: &FailurePacket) -> FailurePacketStatusEntry {
    FailurePacketStatusEntry {
        id: failure_packet_status_id(packet),
        failure_kind: packet.failure_kind.clone(),
        likely_layer: packet.likely_layer.clone(),
        fixture_id: packet.fixture_id.clone(),
        family: packet.family.clone(),
        metric: packet.metric.clone(),
        line: packet.line,
        expected: packet.expected.clone(),
        actual: packet.actual.clone(),
        actual_nearest: packet.nearest_predictions.clone(),
        source_excerpt: packet.source_excerpt.clone(),
        details: packet.details.clone(),
        suggested_next_fix: packet.suggested_next_fix.clone(),
        suggested_next_pr: suggested_next_pr_for_failure_packet(packet),
        first_observed_commit: None,
    }
}

fn failure_packet_status_id(packet: &FailurePacket) -> String {
    let identity = format!(
        "{}|{}|{}|{}|{:?}|{:?}|{:?}|{:?}",
        packet.failure_kind,
        packet.likely_layer,
        packet.fixture_id,
        packet.family.as_deref().unwrap_or(""),
        packet.metric,
        packet.line,
        packet.expected,
        packet.source_excerpt
    );
    format!("packet-{:016x}", stable_hash(&identity))
}

fn suggested_next_pr_for_failure_packet(packet: &FailurePacket) -> String {
    match packet.likely_layer.as_str() {
        "parser" => "fix(parser-core): resolve parser projection failure packet".to_string(),
        "ast_projection" => {
            "feat(parser-accuracy): tighten AST projection fixture expectations".to_string()
        }
        "semantic_fact_extraction" => {
            "fix(semantic): resolve parser-accuracy semantic fact packet".to_string()
        }
        layer => format!("fix(parser-accuracy): resolve {layer} failure packet"),
    }
}

pub(super) fn render_fixture_inventory_status_receipt(
    manifest: &ParserAccuracyManifest,
    artifact: &ParserAccuracyArtifact,
) -> Result<String> {
    let receipt = FixtureInventoryStatusReceipt {
        schema_version: 1,
        commit: artifact.commit.clone(),
        generated_by: "cargo xtask metrics parser-accuracy --export-status-receipts",
        fixture_count: artifact.denominator.fixture_count,
        family_count: artifact.denominator.fixture_family_count,
        scored_lines: artifact.denominator.scored_line_count,
        scored_symbols: artifact.denominator.scored_symbol_count,
        fixtures: manifest.fixtures.iter().map(fixture_inventory_status_entry).collect(),
    };
    render_json_with_newline(&receipt)
}

pub(super) fn render_failure_worklist_status_receipt(artifact: &ParserAccuracyArtifact) -> String {
    let rows = failure_worklist_rows(artifact);
    let mut output = String::new();
    output.push_str("# Parser-accuracy failure worklist\n\n");
    output.push_str(&format!(
        "Source: `target/metrics/parser_accuracy.json` ({} failure packets)\n\n",
        artifact.failure_packets.len()
    ));
    output.push_str("| Family | Count | Likely layer | First fixture | Suggested PR |\n");
    output.push_str("|---|---:|---|---|---|\n");

    if rows.is_empty() {
        output.push_str("| none | 0 | n/a | n/a | n/a |\n");
    } else {
        for row in rows {
            output.push_str(&format!(
                "| {} | {} | {} | `{}` | `{}` |\n",
                row.family, row.count, row.likely_layer, row.first_fixture, row.suggested_pr
            ));
        }
    }

    output
}

pub(super) fn render_next_pointer_status_receipt(artifact: &ParserAccuracyArtifact) -> String {
    let rows = failure_worklist_rows(artifact);
    let mut output = String::new();
    output.push_str("# Parser Accuracy Next\n\n");
    output.push_str("Source: `target/metrics/parser_accuracy.json`\n\n");
    output.push_str(&format!(
        "Denominator: {} fixtures / {} families; {} scored lines; {} scored symbols.\n\n",
        artifact.denominator.fixture_count,
        artifact.denominator.fixture_family_count,
        artifact.denominator.scored_line_count,
        artifact.denominator.scored_symbol_count
    ));
    output.push_str(&format!("Failure packets: {} active.\n\n", artifact.failure_packets.len()));

    if let Some(row) = rows.first() {
        output.push_str("| Field | Value |\n");
        output.push_str("|---|---|\n");
        output.push_str(&format!("| Pointer | `{}` |\n", row.family));
        output.push_str(&format!("| Packet count | {} |\n", row.count));
        output.push_str(&format!("| Likely layer | `{}` |\n", row.likely_layer));
        output.push_str(&format!("| First fixture | `{}` |\n", row.first_fixture));
        output.push_str(&format!("| Suggested PR | `{}` |\n", row.suggested_pr));
        output.push_str(
            "\nUse this pointer only after open measurement/tracking PRs are settled. If the pointed lane has already landed, regenerate this file and take the next row.\n",
        );
    } else {
        output.push_str("Pointer: no active failure packets.\n\n");
        output.push_str("## Next Measurement Gaps\n\n");
        output.push_str("| Metric | Reason | Suggested PR |\n");
        output.push_str("|---|---|---|\n");

        let gaps = next_measurement_gap_rows(artifact);
        let has_no_gaps = gaps.is_empty();
        if has_no_gaps {
            output.push_str("| none | n/a | n/a |\n");
        } else {
            for gap in gaps {
                output.push_str(&format!(
                    "| `{}` | {} | `{}` |\n",
                    markdown_table_cell(&gap.metric),
                    markdown_table_cell(&gap.reason),
                    markdown_table_cell(&gap.suggested_pr)
                ));
            }
        }
        output.push_str(
            "\nUse the measurement gap table only when there are no active failure packets. Keep each measurement gap in its own PR and regenerate this file after a lane lands.\n",
        );
        if has_no_gaps {
            output.push_str("\n## Capability Handoff\n\n");
            output.push_str(
                "Measurement wiring is clear. Follow [`parser.md`](parser.md#raw-failure-buckets) for capability work only when the generated parser status lists a nonzero raw failure bucket. If parser status lists `none`, do not start parser bucket work from stale context; refresh the Linux corpus receipt or move to the next provider or real-workspace trust lane.\n",
            );
        }
    }

    output
}

fn next_measurement_gap_rows(artifact: &ParserAccuracyArtifact) -> Vec<MeasurementGapRow> {
    const MAX_NEXT_GAPS: usize = 5;

    let mut rows: Vec<_> = artifact
        .metrics
        .iter()
        .filter_map(|row| match row {
            MetricRow::InsufficientData { metric, reason, sample_count, .. }
                if *sample_count == 0 =>
            {
                Some(MeasurementGapRow {
                    metric: metric.clone(),
                    reason: reason.clone(),
                    suggested_pr: suggested_next_pr_for_measurement_gap(metric),
                })
            }
            MetricRow::Measured { .. } | MetricRow::InsufficientData { .. } => None,
        })
        .collect();

    rows.sort_by(|left, right| {
        measurement_gap_priority(&left.metric, &left.reason)
            .cmp(&measurement_gap_priority(&right.metric, &right.reason))
            .then_with(|| left.metric.cmp(&right.metric))
    });
    rows.truncate(MAX_NEXT_GAPS);
    rows
}

fn measurement_gap_priority(metric: &str, reason: &str) -> u8 {
    if metric.starts_with("provider_") {
        0
    } else if reason.contains("timing") || metric.ends_with("_ms_p95") {
        1
    } else if reason.contains("telemetry") {
        2
    } else {
        3
    }
}

fn suggested_next_pr_for_measurement_gap(metric: &str) -> String {
    if metric.starts_with("provider_") {
        format!("test(parser-accuracy): wire provider gold fixture for {metric}")
    } else if metric.ends_with("_ms_p95") || metric.ends_with("_query_ms_p95") {
        format!("feat(metrics): wire parser-accuracy timing for {metric}")
    } else {
        format!("feat(metrics): wire parser-accuracy measurement for {metric}")
    }
}

fn markdown_table_cell(value: &str) -> String {
    value.replace('|', "\\|").replace(['\r', '\n'], " ")
}

fn failure_worklist_rows(artifact: &ParserAccuracyArtifact) -> Vec<FailureWorklistRow> {
    let mut rows: BTreeMap<String, FailureWorklistRow> = BTreeMap::new();

    for packet in &artifact.failure_packets {
        let family = packet.failure_kind.clone();
        let suggested_pr = suggested_next_pr_for_failure_packet(packet);
        rows.entry(family.clone())
            .and_modify(|row| {
                row.count += 1;
            })
            .or_insert_with(|| FailureWorklistRow {
                family,
                count: 1,
                likely_layer: packet.likely_layer.clone(),
                first_fixture: packet.fixture_id.clone(),
                suggested_pr,
            });
    }

    let mut rows: Vec<_> = rows.into_values().collect();
    rows.sort_by(|left, right| {
        right.count.cmp(&left.count).then_with(|| left.family.cmp(&right.family))
    });
    rows
}

fn fixture_inventory_status_entry(fixture: &FixtureMetadata) -> FixtureInventoryStatusEntry {
    FixtureInventoryStatusEntry {
        id: fixture.id.clone(),
        family: fixture.family.clone(),
        source_path: fixture.source_path.clone(),
        label_mode: fixture.label_mode,
        scored_lines: fixture.scored_lines,
        scored_symbols: fixture.scored_symbols,
        fully_labeled_regions: fixture.fully_labeled_regions,
        partial_labeled_regions: fixture.partial_labeled_regions,
        unknown_regions: fixture.unknown_regions,
        negative_regions: fixture.negative_regions,
        dynamic_boundaries: fixture.dynamic_boundaries,
        unsupported_constructs: fixture.unsupported_constructs,
        real_project_file: fixture.real_project_file,
        generated: fixture.generated,
        provider_expectation_counts: ProviderExpectationCounts {
            method_completion: fixture.provider_expectations.method_completion.len() as u64,
            diagnostics: fixture.provider_expectations.diagnostics.len() as u64,
            navigation: fixture.provider_expectations.navigation.len() as u64,
        },
    }
}

fn render_json_with_newline(value: &impl Serialize) -> Result<String> {
    let mut rendered = serde_json::to_string_pretty(value)?;
    rendered.push('\n');
    Ok(rendered)
}
