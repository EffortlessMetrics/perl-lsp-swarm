//! `cargo xtask feature-readiness-train packet <node>` — deterministic,
//! content-addressed builder and adversarial reviewer packets for
//! representative feature-readiness nodes (FR-C05 #11286).
//!
//! Offline only: the generator reads its embedded bounded fixture registry
//! plus an optional caller-supplied live snapshot. It invokes no model,
//! schedules no agent, mutates no Git/GitHub state, and writes nothing to the
//! repository; packets are runtime-local stdout outputs.
//!
//! Exit codes: `0` success, `1` validation/instrument failure (fail closed).

pub mod build;
pub mod model;
pub mod nodes;
pub mod render;
pub mod validate;

#[cfg(test)]
mod tests;

use std::path::Path;

use clap::Subcommand;
use color_eyre::eyre::{Context, Result, bail};

use build::LiveSnapshot;
use model::find_node;

/// The feature-readiness train command family over the bounded fixture
/// registry (#11286).
#[derive(Debug, Subcommand)]
pub enum FeatureReadinessTrainCommand {
    /// Emit deterministic packets for one node, or validate the whole
    /// denominator with `--all --check`.
    Packet {
        /// Node id (`fr_8305_import_containment_leaf`), issue number
        /// (`8305` / `#8305`), or unique id prefix. Required unless `--all`.
        node: Option<String>,

        /// Emit the independent adversarial reviewer packet instead of the
        /// builder packet.
        #[arg(long)]
        reviewer: bool,

        /// Phone-readable Markdown projection instead of machine JSON.
        #[arg(long)]
        markdown: bool,

        /// Dense compact projection retaining all load-bearing constraints.
        #[arg(long)]
        compact: bool,

        /// Gate mode: validate instead of printing the full document.
        #[arg(long)]
        check: bool,

        /// Optional exact live snapshot JSON (head/candidate/writer/action
        /// observations — all four keys are required; a deliberately unknown
        /// candidate branch is written as `null`). Without it the packet
        /// states live state unknown and requires a read-only preflight
        /// before any write action.
        #[arg(long = "live-snapshot")]
        live_snapshot: Option<std::path::PathBuf>,

        /// Validate every registry node's builder and reviewer packets with
        /// an in-process determinism proof. Requires `--check`.
        #[arg(long)]
        all: bool,
    },
}

/// Run one feature-readiness train command.
pub fn run(command: FeatureReadinessTrainCommand) -> Result<()> {
    match command {
        FeatureReadinessTrainCommand::Packet {
            node,
            reviewer,
            markdown,
            compact,
            check,
            live_snapshot,
            all,
        } => {
            if markdown && compact {
                bail!("--markdown and --compact are mutually exclusive projections");
            }
            if all {
                if !check {
                    bail!(
                        "refusing an implicit denominator run: pass --check to validate every registry node"
                    );
                }
                if node.is_some() {
                    bail!("--all validates the whole denominator; do not also name a node");
                }
                return run_all(live_snapshot.as_deref());
            }
            let Some(node) = node.as_deref() else {
                bail!(
                    "name a fixture node (id, issue number, or unique prefix), or pass --all --check"
                );
            };
            run_one(node, reviewer, markdown, compact, check, live_snapshot.as_deref())
        }
    }
}

fn load_snapshot(path: Option<&Path>) -> Result<Option<LiveSnapshot>> {
    let Some(path) = path else { return Ok(None) };
    let bytes =
        std::fs::read(path).with_context(|| format!("reading live snapshot {}", path.display()))?;
    Ok(Some(LiveSnapshot::parse(&bytes)?))
}

fn render_selected(doc: &serde_json::Value, markdown: bool, compact: bool) -> String {
    if markdown {
        render::markdown(doc)
    } else if compact {
        render::compact(doc)
    } else {
        render::canonical_json(doc)
    }
}

fn run_one(
    query: &str,
    reviewer: bool,
    markdown: bool,
    compact: bool,
    check: bool,
    snapshot_path: Option<&Path>,
) -> Result<()> {
    let registry_nodes = nodes::all_nodes();
    report_violations(
        "denominator",
        "registry",
        &validate::validate_registry_denominator(&registry_nodes),
    )?;
    let node = find_node(&registry_nodes, query)?;
    let live = load_snapshot(snapshot_path)?;
    let (builder_doc, builder_digest) = build::builder_document(node, live.as_ref());
    let (reviewer_doc, reviewer_digest) = build::reviewer_document(node, live.as_ref());

    let builder_violations = validate::validate_builder(&builder_doc);
    let reviewer_violations = validate::validate_reviewer(&reviewer_doc);
    let pair_violations = validate::validate_pair(&builder_doc, &reviewer_doc);
    report_violations("builder", node.node_id, &builder_violations)?;
    report_violations("reviewer", node.node_id, &reviewer_violations)?;
    report_violations("pair", node.node_id, &pair_violations)?;

    let doc = if reviewer { &reviewer_doc } else { &builder_doc };
    if check {
        let first = render_selected(doc, markdown, compact);
        let second = render_selected(doc, markdown, compact);
        if first != second {
            bail!("non-deterministic render for {}: consecutive renders differ", node.node_id);
        }
        if !markdown && !compact {
            let round_tripped = render::parse_json(&first)
                .with_context(|| format!("round-tripping the packet of {}", node.node_id))?;
            if render::canonical_json(&round_tripped) != first {
                bail!("non-idempotent schema round-trip for {}", node.node_id);
            }
        }
        let compact_text = render::compact(doc);
        if reviewer {
            if compact_text != render::compact(doc) {
                bail!("non-deterministic reviewer compact rendering for {}", node.node_id);
            }
        } else {
            let loss = render::validate_compact_lossless(doc, &compact_text);
            report_violations("compact", node.node_id, &loss)?;
        }
        // Both generated documents are bound to the receipt with labeled
        // roles: a persisted reviewer-check line must stay attached to the
        // reviewer bytes it validated, never to the builder digest.
        println!(
            "{}",
            check_receipt_line(
                node.node_id,
                if reviewer { "reviewer" } else { "builder" },
                &builder_digest,
                &reviewer_digest,
            )
        );
        return Ok(());
    }
    print!("{}", render_selected(doc, markdown, compact));
    Ok(())
}

/// The `--check` receipt: one deterministic line binding both packet digests
/// to their roles so downstream consumers cannot attach a check result to the
/// wrong document.
fn check_receipt_line(
    node_id: &str,
    selected_packet: &str,
    builder_digest: &str,
    reviewer_digest: &str,
) -> String {
    format!(
        "FR_PACKET_CHECK node={node_id} packet={selected_packet} \
         builder_digest={builder_digest} reviewer_digest={reviewer_digest} status=ok"
    )
}

fn run_all(snapshot_path: Option<&Path>) -> Result<()> {
    // The denominator gate consumes the same snapshot evidence as a
    // single-node check: an unreadable or incomplete snapshot fails closed
    // here instead of silently degrading to an offline run.
    let live = load_snapshot(snapshot_path)?;
    let registry_nodes = nodes::all_nodes();
    report_violations(
        "denominator",
        "registry",
        &validate::validate_registry_denominator(&registry_nodes),
    )?;
    let mut checked = 0usize;
    for node in &registry_nodes {
        let (builder_doc, builder_digest) = build::builder_document(node, live.as_ref());
        let (reviewer_doc, _reviewer_digest) = build::reviewer_document(node, live.as_ref());
        report_violations("builder", node.node_id, &validate::validate_builder(&builder_doc))?;
        report_violations("reviewer", node.node_id, &validate::validate_reviewer(&reviewer_doc))?;
        report_violations(
            "pair",
            node.node_id,
            &validate::validate_pair(&builder_doc, &reviewer_doc),
        )?;
        // Determinism: two independent builds of the same node must agree
        // byte-for-byte, including content-addressed identity.
        let rebuilt = build::builder_document(node, live.as_ref());
        if render::canonical_json(&rebuilt.0) != render::canonical_json(&builder_doc) {
            bail!("non-deterministic builder generation for {}", node.node_id);
        }
        let rebuilt_reviewer = build::reviewer_document(node, live.as_ref());
        if render::canonical_json(&rebuilt_reviewer.0) != render::canonical_json(&reviewer_doc) {
            bail!("non-deterministic reviewer generation for {}", node.node_id);
        }
        let compact_text = render::compact(&builder_doc);
        report_violations(
            "compact",
            node.node_id,
            &render::validate_compact_lossless(&builder_doc, &compact_text),
        )?;
        let reviewer_compact = render::compact(&reviewer_doc);
        if reviewer_compact != render::compact(&reviewer_doc) {
            bail!("non-deterministic reviewer compact rendering for {}", node.node_id);
        }
        println!(
            "FR_PACKET node={} builder={} role={} status=ok",
            node.node_id,
            builder_digest.get(..16).unwrap_or_default(),
            node.role.as_str()
        );
        checked += 1;
    }
    let actionable = nodes::denominator()
        .iter()
        .filter(|entry| entry.disposition == model::DenominatorDisposition::Actionable)
        .count();
    let deferred = nodes::denominator()
        .iter()
        .filter(|entry| entry.disposition == model::DenominatorDisposition::Deferred)
        .count();
    let excluded = nodes::denominator()
        .iter()
        .filter(|entry| entry.disposition == model::DenominatorDisposition::Excluded)
        .count();
    println!(
        "FR_PACKET_DENOMINATOR actionable={actionable} deferred={deferred} excluded={excluded} duplicate=0 checked={checked} status=ok"
    );
    Ok(())
}

fn report_violations(plane: &str, node_id: &str, violations: &[validate::Violation]) -> Result<()> {
    use std::fmt::Write as _;
    if violations.is_empty() {
        return Ok(());
    }
    let mut detail = String::new();
    for violation in violations {
        let _ = writeln!(detail, "  [{}] {}: {}", plane, violation.code, violation.detail);
    }
    bail!(
        "packet validation failed for {} ({plane}, {} violations):\n{}",
        node_id,
        violations.len(),
        detail.trim_end()
    );
}
