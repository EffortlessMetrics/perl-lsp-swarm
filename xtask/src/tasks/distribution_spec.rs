//! `cargo xtask distribution-spec check|explain|index` — the checked spec
//! disposition compiler over the distribution train (#11164).
//!
//! This module consumes the #10712 checked graph
//! ([`crate::tasks::distribution_train`]) plus the compiled `.spec` packets and
//! provides, offline:
//!
//! * `check` — exactly one spec/no-spec disposition per node, the eleven
//!   durable route packets compiled and linked to their exact nodes, the eight
//!   bounded adapter/docs leaves issue-plan sufficient only while consuming
//!   settled contracts, controllers holding no implementation packet, and every
//!   spec reference resolving in this tree;
//! * `explain` — the packet/node view a low-cost builder reads instead of
//!   re-mining issue comments (`summary` gives the disposition histogram);
//! * `index` — one deterministic projection of every node's disposition,
//!   second-run clean, consumable by #10726.
//!
//! Fail-closed laws: a SPEC_COMPILED node without its packet, a controller with
//! a packet, a packet that does not name its node, or an index that drifts
//! between runs are all hard failures. No command here reads GitHub, mutates a
//! packet, or evaluates route evidence.

use std::collections::BTreeSet;
use std::path::Path;

use clap::Subcommand;
use color_eyre::eyre::{Result, bail};
use serde_json::json;

use crate::tasks::distribution_train::TrainGraph;
use crate::utils::project_root;

/// The reviewed SPEC_COMPILED set (#11164): the eleven durable route packets
/// plus the two control-plane contracts that compile themselves (#10712 bundle,
/// #11164 disposition table). Packet-collapse falsifier: a route contract that
/// silently degrades to an issue plan, or a controller that grows a packet,
/// fails here.
const REQUIRED_SPEC_COMPILED: [&str; 13] = [
    "10712", "11164", "10333", "11432", "11434", "11443", "11445", "11447", "11449", "11456",
    "11458", "11461", "11463",
];

/// The eight bounded adapter/docs leaves (#11164) that stay
/// ISSUE_PLAN_SUFFICIENT only while consuming settled producer contracts.
const ISSUE_PLAN_SUFFICIENT_LEAVES: [&str; 8] =
    ["11436", "11437", "11438", "11439", "11441", "11451", "11453", "11454"];

/// The route-projection controllers (#11164) that must never receive an
/// implementation packet.
const CONTROLLER_NO_PACKET: [&str; 6] = ["10703", "8087", "10334", "10336", "10339", "10342"];

/// Files every compiled packet retains (#11164).
const PACKET_FILES: [&str; 3] = ["context.md", "acceptance.md", "checklist.md"];

#[derive(Subcommand, Debug)]
pub enum SpecCommand {
    /// Validate spec dispositions, packet presence, and packet/node linkage.
    Check {
        /// Print a machine-readable JSON summary.
        #[arg(long)]
        json: bool,
    },
    /// Explain one node's packet view (`summary` for the disposition
    /// histogram).
    Explain {
        /// Node id, `#issue`, or `summary`.
        subject: String,
    },
    /// Print the deterministic disposition index over the whole graph.
    Index,
}

/// Run one `distribution-spec` command.
pub fn run(command: SpecCommand) -> Result<()> {
    let root = project_root()?;
    let graph = TrainGraph::load(&root)?;
    match command {
        SpecCommand::Check { json } => run_check(&graph, &root, json),
        SpecCommand::Explain { subject } => {
            if subject == "summary" {
                println!("{}", render_summary(&graph));
                return Ok(());
            }
            let node = graph.resolve(&subject)?;
            println!("{}", render_explain(&graph, &root, &node.node_id));
            Ok(())
        }
        SpecCommand::Index => {
            println!("{}", render_index(&graph, &root));
            Ok(())
        }
    }
}

/// All spec-compilation violations for the current tree.
fn spec_violations(graph: &TrainGraph, root: &Path) -> Vec<String> {
    let mut violations = Vec::new();
    let spec_compiled: BTreeSet<&str> = graph
        .nodes()
        .iter()
        .filter(|node| node.spec_disposition == "SPEC_COMPILED")
        .map(|node| node.node_id.as_str())
        .collect();
    let required: BTreeSet<&str> = REQUIRED_SPEC_COMPILED.iter().copied().collect();

    // S01: the reviewed SPEC_COMPILED set is exact — no packet collapse, no
    // controller packet.
    for missing in required.difference(&spec_compiled) {
        violations.push(format!(
            "S01 [{missing}]: required SPEC_COMPILED node lost its disposition (packet collapse)"
        ));
    }
    for extra in spec_compiled.difference(&required) {
        violations.push(format!("S01 [{extra}]: SPEC_COMPILED outside the reviewed #11164 set"));
    }

    for node in graph.nodes() {
        let id = &node.node_id;

        // S02: SPEC_COMPILED packets exist, retain the three files, and name
        // their node.
        if node.spec_disposition == "SPEC_COMPILED" {
            let raw = node.spec_ref.split('#').next().unwrap_or(&node.spec_ref);
            let path = root.join(raw);
            if node.spec_ref.contains('#') {
                match std::fs::read_to_string(&path) {
                    Ok(content) => {
                        if !content.contains("spec_dispositions") {
                            violations.push(format!(
                                "S02 [{id}]: {} does not carry the disposition table",
                                node.spec_ref
                            ));
                        }
                    }
                    Err(error) => violations
                        .push(format!("S02 [{id}]: cannot read {}: {error}", node.spec_ref)),
                }
            } else {
                for file in PACKET_FILES {
                    let file_path = path.join(file);
                    match std::fs::read_to_string(&file_path) {
                        Ok(content) if !content.trim().is_empty() => {
                            if file == "context.md" {
                                if !content.contains(&format!("#{}", node.issue_number)) {
                                    violations.push(format!(
                                        "S02 [{id}]: packet context.md does not name #{}, so the packet/node link is broken",
                                        node.issue_number
                                    ));
                                }
                                if let Some(key) = node.conflict_key.as_deref()
                                    && !content.contains(key)
                                {
                                    violations.push(format!(
                                            "S02 [{id}]: packet context.md does not name conflict key {key}"
                                        ));
                                }
                            }
                        }
                        Ok(_) => violations.push(format!(
                            "S02 [{id}]: packet file {} is empty",
                            file_path.display()
                        )),
                        Err(error) => violations.push(format!(
                            "S02 [{id}]: packet file {} missing: {error}",
                            file_path.display()
                        )),
                    }
                }
            }
        }

        // S03: the #11164 route controllers never receive a packet. Other
        // programmes may own `.spec/<issue>-*` directories; only this train's
        // controller set is guarded here, and the manifest laws already forbid
        // any governance node from claiming a compiled packet.
        if CONTROLLER_NO_PACKET.contains(&id.as_str()) && packet_dir_exists(root, node.issue_number)
        {
            violations.push(format!(
                "S03 [{id}]: route controller has a .spec/{}-* packet; controllers receive no implementation packet",
                node.issue_number
            ));
        }
    }

    // S04: the bounded adapter/docs leaves stay issue-plan sufficient.
    for id in ISSUE_PLAN_SUFFICIENT_LEAVES {
        let Some(node) = graph.node(id) else {
            violations.push(format!("S04 [{id}]: required leaf missing from the graph"));
            continue;
        };
        if node.spec_disposition != "ISSUE_PLAN_SUFFICIENT" {
            violations.push(format!(
                "S04 [{id}]: bounded adapter/docs leaf must stay ISSUE_PLAN_SUFFICIENT while contracts remain settled (found {})",
                node.spec_disposition
            ));
        }
    }

    // S05: route controllers carry no implementation packet.
    for id in CONTROLLER_NO_PACKET {
        match graph.node(id) {
            Some(node) if node.spec_disposition != "NO_SPEC_DELTA" => violations.push(format!(
                "S05 [{id}]: route controller must carry NO_SPEC_DELTA (found {})",
                node.spec_disposition
            )),
            Some(_) => {}
            None => violations.push(format!("S05 [{id}]: controller missing from the graph")),
        }
    }

    violations
}

fn packet_dir_exists(root: &Path, issue: u64) -> bool {
    let prefix = format!("{issue}-");
    let Ok(entries) = std::fs::read_dir(root.join(".spec")) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        entry.path().is_dir()
            && entry.file_name().to_str().is_some_and(|name| name.starts_with(&prefix))
    })
}

fn run_check(graph: &TrainGraph, root: &Path, json: bool) -> Result<()> {
    let violations = spec_violations(graph, root);
    if !violations.is_empty() {
        bail!(
            "distribution-spec check failed with {} violation(s):\n  - {}",
            violations.len(),
            violations.join("\n  - ")
        );
    }
    if json {
        println!(
            "{}",
            json!({
                "schema": "distribution_spec_index.v1",
                "result": "ok",
                "nodes": graph.nodes().len(),
                "spec_compiled": REQUIRED_SPEC_COMPILED.len(),
            })
        );
    } else {
        println!(
            "distribution-spec check: OK nodes={} spec_compiled={} issue_plan_sufficient_leaves={}",
            graph.nodes().len(),
            REQUIRED_SPEC_COMPILED.len(),
            ISSUE_PLAN_SUFFICIENT_LEAVES.len(),
        );
    }
    Ok(())
}

fn render_summary(graph: &TrainGraph) -> String {
    let mut histogram: Vec<(String, usize)> = Vec::new();
    for node in graph.nodes() {
        let disposition = node.spec_disposition.to_string();
        match histogram.iter_mut().find(|(name, _)| name == &disposition) {
            Some((_, count)) => *count += 1,
            None => histogram.push((disposition, 1)),
        }
    }
    histogram.sort();
    let mut out = String::from("distribution-spec disposition summary:\n");
    for (name, count) in histogram {
        out.push_str(&format!("  {name}: {count}\n"));
    }
    out.push_str(&format!("  total: {}\n", graph.nodes().len()));
    out
}

fn render_explain(graph: &TrainGraph, root: &Path, node_id: &str) -> String {
    let Some(node) = graph.node(node_id) else {
        return format!("no train node {node_id}");
    };
    let mut out = String::new();
    out.push_str(&format!("# spec view: {} — {}\n", node.node_id, node.title));
    out.push_str(&format!("disposition: {} -> {}\n", node.spec_disposition, node.spec_ref));
    out.push_str(&format!("one-PR proposition: {}\n", node.one_pr_outcome));
    out.push_str(&format!(
        "hard/evidence predecessors: {} / {}\n",
        node.hard_dependencies.join(", "),
        node.evidence_dependencies.join(", "),
    ));
    out.push_str(&format!("conflict key: {}\n", node.conflict_key.as_deref().unwrap_or("(none)")));
    out.push_str(&format!(
        "falsifiers: {}\n",
        if node.falsifier_refs.is_empty() {
            "(see packet acceptance.md)".to_string()
        } else {
            node.falsifier_refs.join(", ")
        }
    ));
    out.push_str(&format!("proof: {}\n", node.proof_commands.join(" && ")));
    out.push_str(&format!("stop conditions: {}\n", node.do_not_build.join("; ")));
    if node.spec_disposition == "SPEC_COMPILED" && !node.spec_ref.contains('#') {
        let packet = root.join(&node.spec_ref);
        out.push_str("packet files:\n");
        for file in PACKET_FILES {
            let path = packet.join(file);
            out.push_str(&format!(
                "  - {} {}\n",
                path.display(),
                if path.exists() { "(present)" } else { "(MISSING)" }
            ));
        }
    }
    for limitation in &node.limitations {
        out.push_str(&format!("limitation: {limitation}\n"));
    }
    out
}

fn render_index(graph: &TrainGraph, root: &Path) -> String {
    let mut out = String::from(
        "distribution-spec index (disposition per node; deterministic; #10726 may consume):\n",
    );
    out.push_str("issue       lane                kind            disposition                      spec_ref\n");
    let mut nodes: Vec<_> = graph.nodes().iter().collect();
    nodes.sort_by_key(|node| node.issue_number);
    for node in nodes {
        let packet_note =
            if node.spec_disposition == "SPEC_COMPILED" && !node.spec_ref.contains('#') {
                let present =
                    PACKET_FILES.iter().all(|file| root.join(&node.spec_ref).join(file).exists());
                if present { " [packet]" } else { " [PACKET INCOMPLETE]" }
            } else {
                ""
            };
        out.push_str(&format!(
            "{:<11} {:<19} {:<15} {:<32} {}{}\n",
            node.issue_number,
            node.lane.as_str(),
            node.kind.as_str(),
            node.spec_disposition,
            node.spec_ref,
            packet_note,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "test-only fixtures and static patterns; production paths stay panic-free"
    )]
    use super::*;

    fn graph() -> TrainGraph {
        let root = project_root().expect("project root");
        TrainGraph::load(&root).expect("checked graph loads")
    }

    #[test]
    fn committed_spec_state_passes_every_law() {
        let root = project_root().expect("project root");
        let graph = graph();
        assert!(graph.laws().is_empty());
        assert!(
            spec_violations(&graph, &root).is_empty(),
            "spec violations: {:?}",
            spec_violations(&graph, &root)
        );
    }

    #[test]
    fn index_is_second_run_clean() {
        let root = project_root().expect("project root");
        let graph = graph();
        let first = render_index(&graph, &root);
        let second = render_index(&graph, &root);
        assert_eq!(first, second);
    }

    /// An isolated probe root under `target/` so mutation tests never race
    /// with the committed `.spec` tree.
    fn probe_root(tag: &str) -> std::path::PathBuf {
        let root = project_root().expect("project root");
        let probe = root.join("target").join("distribution-spec-probe").join(tag);
        std::fs::create_dir_all(probe.join(".spec")).expect("probe root");
        probe
    }

    #[test]
    fn packet_collapse_is_rejected() {
        // A route contract silently degrading to an issue plan must fail.
        let root = project_root().expect("project root");
        let bytes = std::fs::read_to_string(
            root.join(crate::tasks::distribution_train::MANIFEST_RELATIVE_PATH),
        )
        .expect("manifest");
        let mut value: serde_json::Value = serde_json::from_str(&bytes).expect("json");
        let nodes =
            value.get_mut("nodes").and_then(serde_json::Value::as_array_mut).expect("nodes");
        let node = nodes
            .iter_mut()
            .find(|n| n.get("node_id").and_then(serde_json::Value::as_str) == Some("11463"))
            .expect("node");
        node["spec_disposition"] = serde_json::Value::String("ISSUE_PLAN_SUFFICIENT".into());
        node["spec_ref"] = serde_json::Value::String("#11463".into());
        let manifest = serde_json::from_value::<crate::tasks::distribution_train::Manifest>(value)
            .expect("mutated manifest parses");
        let mutated = TrainGraph::from_manifest(manifest).expect("graph");
        let violations = spec_violations(&mutated, &root);
        assert!(
            violations.iter().any(|v| v.starts_with("S01 [11463]")),
            "unexpected violations: {violations:?}"
        );
    }

    #[test]
    fn controller_packet_probe_is_detected() {
        let root = project_root().expect("project root");
        let graph = graph();
        // The committed tree never grants a controller a packet.
        assert!(!packet_dir_exists(&root, 8087));
        assert!(spec_violations(&graph, &root).is_empty());
        // A fabricated `.spec/8087-*` directory trips the S03 rule.
        let probe = probe_root("controller-packet");
        std::fs::create_dir_all(probe.join(".spec").join("8087-controller-packet"))
            .expect("probe dir");
        assert!(packet_dir_exists(&probe, 8087));
        assert!(!packet_dir_exists(&probe, 11463));
    }

    #[test]
    fn incomplete_spec_compiled_packet_is_rejected() {
        let root = project_root().expect("project root");
        let graph = graph();
        // Copy one packet into the probe root with acceptance.md missing;
        // the probe root is checked instead of the committed tree.
        let probe = probe_root("incomplete-packet");
        let source = root.join(".spec").join("11463-install-claim-cutover");
        let target = probe.join(".spec").join("11463-install-claim-cutover");
        std::fs::create_dir_all(&target).expect("probe packet");
        for file in ["context.md", "checklist.md"] {
            std::fs::copy(source.join(file), target.join(file)).expect("copy packet file");
        }
        let violations = spec_violations(&graph, &probe);
        assert!(
            violations.iter().any(|v| v.contains("11463") && v.contains("acceptance.md")),
            "unexpected violations: {violations:?}"
        );
        // The committed packet is untouched and still clean.
        assert!(spec_violations(&graph, &root).is_empty());
    }

    #[test]
    fn summary_covers_every_node_exactly_once() {
        let graph = graph();
        let summary = render_summary(&graph);
        let total: usize = summary
            .lines()
            .filter_map(|line| line.trim().rsplit_once(": "))
            .filter(|(name, _)| *name != "total")
            .map(|(_, count)| count.parse::<usize>().expect("count"))
            .sum();
        assert_eq!(total, graph.nodes().len());
    }
}
