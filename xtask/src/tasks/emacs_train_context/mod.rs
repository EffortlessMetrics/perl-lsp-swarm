//! `cargo xtask integration emacs train context` — the exact-tree context
//! engine of the Emacs support train (CTXENG `#11756`, populating the E04
//! context plane `#11718`).
//!
//! Deterministic and offline only: the engine reads the stable
//! `emacs_train.v1` manifest (E01 `#10918`), the `emacs_train_revision.v1`
//! ledger (E01R `#11770`) and its own population mapping document, validates
//! every claim against the exact current tree, and emits one bounded
//! `emacs_node_context.v1` packet per node. It never reads or writes GitHub,
//! never executes product commands, and never caches packets: every run
//! re-derives, and every packet embeds the digests that make cross-tree reuse
//! detectable.
//!
//! Population of the full Emacs node denominator belongs to the population
//! leaves (#11757 substrate, #11758 projection); nodes without a mapping
//! entry resolve to a precise typed blocker (`mapping_gap`), never to a
//! guessed path or a silent skip.
//!
//! Exit codes: `0` full context, `3` precise mapping-gap packet (still
//! printed), `1` instrument/law failure (fail closed, no packet).

pub mod digest;
pub mod model;
pub mod render;
pub mod resolve;

#[cfg(test)]
mod tests;

use std::path::Path;

use clap::Subcommand;
use color_eyre::eyre::{Context, Result, bail};

use render::{render_json, render_markdown};
use resolve::{Resolution, load_inputs, resolve_node, resolve_spec};

#[derive(Debug, Subcommand)]
pub enum EmacsTrainCommand {
    /// Emit the bounded exact-tree context packet for one node (node id or
    /// issue number). Recomputes everything from manifest + ledger + mapping
    /// + exact tree on every run.
    Context {
        /// Node id (e.g. `CTXENG`) or issue number (`11756` / `#11756`).
        node: String,
        /// Output format: `json` (packet) or `markdown` (navigation view).
        #[arg(long, default_value = "json")]
        format: String,
    },
    /// Validate the whole context denominator: every stable node must either
    /// resolve to a fully validated packet or produce a precise typed
    /// mapping blocker. Also proves in-process determinism by rendering
    /// every packet twice and comparing bytes. Requires `--check` so the
    /// gate is always an explicit invocation.
    Contexts {
        /// Run the denominator validation.
        #[arg(long)]
        check: bool,
        /// Additionally fail on mapping gaps instead of accepting precise
        /// blockers (population-completeness gate for #11757/#11758).
        #[arg(long)]
        strict: bool,
    },
}

pub fn run(command: EmacsTrainCommand) -> Result<()> {
    let root = crate::utils::project_root()
        .with_context(|| "locating the repository root for the emacs train context engine")?;
    match command {
        EmacsTrainCommand::Context { node, format } => run_context(&root, &node, &format),
        EmacsTrainCommand::Contexts { check, strict } => {
            if !check {
                bail!(
                    "refusing an implicit denominator run: pass --check to validate that every \
                     stable node resolves to a bounded packet or a precise typed blocker"
                );
            }
            run_contexts(&root, strict)
        }
    }
}

fn run_context(root: &Path, node: &str, format: &str) -> Result<()> {
    let inputs = load_inputs(root)?;
    let resolution = resolve_spec(root, &inputs, node)?;
    let packet = resolution.packet();
    let rendered = match format {
        "json" => render_json(packet)?,
        "markdown" | "md" => render_markdown(packet),
        other => bail!("unknown format '{other}': expected `json` or `markdown`"),
    };
    println!("{rendered}");
    if resolution.is_gap() {
        // A precise blocker is a valid answer for a fresh agent, but must be
        // distinguishable from a full context by any caller.
        std::process::exit(3);
    }
    Ok(())
}

pub(crate) fn run_contexts_at(root: &Path, strict: bool) -> Result<()> {
    let inputs = load_inputs(root)?;
    let mut mapped = 0usize;
    let mut gaps = 0usize;
    let mut gap_lines: Vec<String> = Vec::new();
    for node in &inputs.manifest.nodes {
        let resolution = resolve_node(root, &inputs, node)
            .with_context(|| format!("resolving context for node {}", node.node_id))?;
        // In-process determinism and schema round-trip proof: two
        // independent renders must agree byte-for-byte and re-parse through
        // the packet schema for every node in the denominator.
        let first = render_json(resolution.packet())?;
        let second = render_json(resolution.packet())?;
        if first != second {
            bail!(
                "non-deterministic render for node {}: two consecutive renders of the same \
                 resolution differ",
                node.node_id
            );
        }
        let round_tripped = render::parse_json(&first)
            .with_context(|| format!("round-tripping the packet of node {}", node.node_id))?;
        if render_json(&round_tripped)? != first {
            bail!(
                "non-idempotent schema round-trip for node {}: re-serializing the parsed \
                 packet changed bytes",
                node.node_id
            );
        }
        match resolution {
            Resolution::Packet(_) => {
                mapped += 1;
                println!(
                    "EMU_CONTEXT node={} status=ok components={} tests={} reads={} writes={}",
                    node.node_id,
                    resolution.packet().components.len(),
                    resolution.packet().tests.len(),
                    resolution.packet().read_set.len(),
                    resolution.packet().write_set.len()
                );
            }
            Resolution::Gap(_) => {
                gaps += 1;
                let gap = resolution
                    .packet()
                    .gaps
                    .first()
                    .map(|gap| (gap.reason.clone(), gap.owner_issue))
                    .unwrap_or_else(|| ("unknown".to_owned(), 0));
                gap_lines.push(format!(
                    "EMU_CONTEXT node={} status=mapping_gap owner=#{} reason=\"{}\"",
                    node.node_id, gap.1, gap.0
                ));
            }
        }
    }
    for line in gap_lines {
        println!("{line}");
    }
    println!(
        "EMU_CONTEXTS_CHECK=OK mapped={mapped} mapping_gap={gaps} total={} manifest_sha256={} \
         ledger_sha256={} tree={}",
        inputs.manifest.nodes.len(),
        inputs.manifest_digest,
        inputs.ledger_digest,
        inputs.git_tree
    );
    if strict && gaps > 0 {
        bail!(
            "strict denominator check failed: {gaps} node(s) carry mapping gaps; population is \
             owned by #11757/#11758"
        );
    }
    Ok(())
}

fn run_contexts(root: &Path, strict: bool) -> Result<()> {
    run_contexts_at(root, strict)
}
