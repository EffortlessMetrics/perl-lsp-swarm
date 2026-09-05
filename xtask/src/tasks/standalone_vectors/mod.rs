//! Standalone install semantic conformance vectors (#11550, child 1 of
//! #10737): deterministic corpus, independent expected-outcome oracle,
//! fixture-port protocol data, and the mutation bank.
//!
//! Proof-only by construction: this harness never executes the production
//! POSIX/PowerShell adapters (an import/subprocess boundary test pins that),
//! implements no product behavior, and routes any future adapter mismatch
//! to the runner children #11551/#11552/#11554.
//!
//! Commands:
//!
//! ```text
//! cargo xtask standalone-vectors check            [--update-golden]
//! cargo xtask standalone-vectors explain <vector>
//! cargo xtask standalone-vectors mutation-check
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Context, Result, bail, eyre};

use crate::utils::project_root;

use self::mutations::MUTATION_BANK;
use self::oracle::{Deviation, OracleError};
use self::schema::{CORPUS_SCHEMA_ID, CorpusManifest, PlatformClassification, StageId, Vector};

pub mod mutations;
pub mod oracle;
pub mod schema;

/// Corpus directory relative to the repository root.
pub const CORPUS_DIR: &str = "fixtures/standalone_install_vectors";
/// Golden packet directory relative to [`CORPUS_DIR`].
pub const GOLDEN_DIR: &str = "expected";
/// Live-truth literal that must never be copied into the corpus (#11550
/// negative control: no current topology/channel/public truth).
const LIVE_REPO_SLUG: &str = "EffortlessMetrics/perl-lsp";

/// Loads and structurally validates the whole corpus.
pub fn load_corpus() -> Result<Vec<Vector>> {
    let root = project_root()?;
    let dir = root.join(CORPUS_DIR);
    let manifest_path = dir.join("corpus.v1.json");
    let manifest_text = fs::read_to_string(&manifest_path)
        .with_context(|| format!("reading corpus manifest {}", manifest_path.display()))?;
    let manifest: CorpusManifest = parse_strict(&manifest_text)
        .with_context(|| format!("parsing corpus manifest {}", manifest_path.display()))?;

    if manifest.schema != CORPUS_SCHEMA_ID {
        bail!("corpus manifest declares schema {:?}, expected {CORPUS_SCHEMA_ID}", manifest.schema);
    }

    // Manifest order is stable and duplicate-free.
    let mut seen = BTreeSet::new();
    for reference in &manifest.vectors {
        if !seen.insert(reference.vector_id.clone()) {
            bail!("duplicate vector_id {:?} in corpus manifest", reference.vector_id);
        }
    }
    let declared_order: Vec<&String> =
        manifest.vectors.iter().map(|reference| &reference.vector_id).collect();
    let mut sorted_order = declared_order.clone();
    sorted_order.sort();
    if sorted_order != declared_order {
        bail!("corpus manifest vectors must be listed in ascending vector_id order");
    }

    let mut vectors = Vec::with_capacity(manifest.vectors.len());
    for reference in &manifest.vectors {
        let path = dir.join(&reference.path);
        let text = fs::read_to_string(&path)
            .with_context(|| format!("reading vector {}", path.display()))?;
        let vector: Vector =
            parse_strict(&text).with_context(|| format!("parsing vector {}", path.display()))?;
        if vector.vector_id != reference.vector_id {
            bail!(
                "vector file {} declares id {:?} but the manifest lists {:?}",
                path.display(),
                vector.vector_id,
                reference.vector_id
            );
        }
        if vector.contract_generation != manifest.contract_generation {
            bail!(
                "vector {:?} generation {} != manifest generation {}",
                vector.vector_id,
                vector.contract_generation,
                manifest.contract_generation
            );
        }
        vectors.push(vector);
    }

    scan_live_truth(&root, &dir, &vectors)?;
    Ok(vectors)
}

/// Strict JSON decode: unknown fields are corpus errors, never ignored data.
fn parse_strict<T: serde::de::DeserializeOwned>(text: &str) -> Result<T> {
    serde_json::from_str(text).map_err(|error| eyre!("{error}"))
}

/// Rejects copied live truth: the real repository slug and the current
/// workspace version may not appear anywhere in the corpus fixtures.
fn scan_live_truth(root: &Path, dir: &Path, vectors: &[Vector]) -> Result<()> {
    let cargo_toml = fs::read_to_string(root.join("Cargo.toml"))
        .with_context(|| "reading workspace Cargo.toml".to_string())?;
    let workspace_version = workspace_version_from_cargo_toml(&cargo_toml)
        .ok_or_else(|| eyre!("could not resolve [workspace.package] version"))?;
    let mut offenders = Vec::new();

    let manifest_text = fs::read_to_string(dir.join("corpus.v1.json"))
        .with_context(|| "re-reading corpus manifest".to_string())?;
    for literal in [LIVE_REPO_SLUG, workspace_version.as_str()] {
        if manifest_text.contains(literal) {
            offenders.push(format!("corpus.v1.json contains live-truth literal {literal:?}"));
        }
    }
    for vector in vectors {
        let serialized = serde_json::to_string(vector).unwrap_or_default();
        for literal in [LIVE_REPO_SLUG, workspace_version.as_str()] {
            if serialized.contains(literal) {
                offenders.push(format!(
                    "vector {:?} contains live-truth literal {literal:?}",
                    vector.vector_id
                ));
            }
        }
    }
    if !offenders.is_empty() {
        bail!("live-truth scan failed:\n  {}", offenders.join("\n  "));
    }
    Ok(())
}

/// Extracts `[workspace.package].version` without pulling a TOML dependency.
fn workspace_version_from_cargo_toml(cargo_toml: &str) -> Option<String> {
    let mut in_workspace_package = false;
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_workspace_package = trimmed == "[workspace.package]";
            continue;
        }
        if !in_workspace_package {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("version = ") {
            let quoted = value.trim();
            if quoted.len() >= 2 && quoted.starts_with('"') && quoted.ends_with('"') {
                return Some(quoted[1..quoted.len() - 1].to_string());
            }
        }
    }
    None
}

/// Canonical byte form of a derived packet (stable across platforms).
fn packet_bytes(packet: &oracle::SemanticPacket) -> Result<Vec<u8>> {
    let mut rendered = serde_json::to_string_pretty(packet)
        .with_context(|| format!("serializing packet for {:?}", packet.vector_id))?;
    rendered.push('\n');
    Ok(rendered.into_bytes())
}

fn golden_path(vector_id: &str) -> Result<PathBuf> {
    let root = project_root()?;
    Ok(root.join(CORPUS_DIR).join(GOLDEN_DIR).join(format!("{vector_id}.json")))
}

fn read_golden_normalized(vector_id: &str) -> Result<Option<String>> {
    let path = golden_path(vector_id)?;
    match fs::read_to_string(&path) {
        Ok(text) => Ok(Some(normalize_newlines(&text))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(eyre!("reading golden {}: {error}", path.display())),
    }
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// One failed expectation binding a vector field to derived reality.
struct Mismatch {
    vector_id: String,
    detail: String,
}

/// Field-by-field comparison between authored expectations and the derived
/// packet. Every compared field is part of the #10737 semantic assertions.
fn consequence_mismatches(vector: &Vector, packet: &oracle::SemanticPacket) -> Vec<Mismatch> {
    let expected = &vector.expected;
    let id = vector.vector_id.clone();
    let mut out = Vec::new();
    let mut push = |field: &str, ok: bool, detail: String| {
        if !ok {
            out.push(Mismatch { vector_id: id.clone(), detail: format!("{field}: {detail}") });
        }
    };

    push(
        "terminal.result",
        packet.terminal.result == expected.terminal_result,
        format!("expected {:?} got {:?}", expected.terminal_result, packet.terminal.result),
    );
    push(
        "terminal.stage",
        packet.terminal.stage_id == expected.terminal_stage,
        format!(
            "expected {} got {}",
            stage_display(expected.terminal_stage),
            stage_display(packet.terminal.stage_id)
        ),
    );
    push(
        "terminal.reason",
        packet.terminal.reason_family == expected.reason_family,
        format!("expected {:?} got {:?}", expected.reason_family, packet.terminal.reason_family),
    );
    push(
        "terminal.action",
        packet.terminal.action_class == expected.action_class,
        format!("expected {:?} got {:?}", expected.action_class, packet.terminal.action_class),
    );
    push(
        "side_effect_ceiling",
        packet.side_effect_ceiling == expected.side_effect_ceiling,
        format!("expected {:?} got {:?}", expected.side_effect_ceiling, packet.side_effect_ceiling),
    );
    push(
        "claim_ceiling",
        packet.claim_ceiling == expected.claim_ceiling,
        format!("expected {:?} got {:?}", expected.claim_ceiling, packet.claim_ceiling),
    );
    push(
        "pair_claims_satisfied",
        packet.pair_claims_satisfied == expected.pair_claims_satisfied,
        format!("expected {} got {}", expected.pair_claims_satisfied, packet.pair_claims_satisfied),
    );
    push(
        "branch_count",
        packet.branches.len() == expected.branch_count,
        format!("expected {} got {}", expected.branch_count, packet.branches.len()),
    );
    push(
        "attempt_count",
        packet.attempts.len() == expected.attempt_count,
        format!("expected {} got {}", expected.attempt_count, packet.attempts.len()),
    );
    out
}

pub(crate) fn stage_display(stage: StageId) -> &'static str {
    oracle::stage_display_name(stage)
}

/// Runs the full corpus check: schema, rules, derivations, headline
/// assertions, golden determinism, and (optionally) rewrites goldens as an
/// explicit writer action.
pub fn run_check(update_golden: bool) -> Result<()> {
    let vectors = load_corpus()?;
    let mut mismatches: Vec<Mismatch> = Vec::new();
    let mut regenerated = 0usize;

    for vector in &vectors {
        let packet = match oracle::derive_packet(vector, Deviation::None) {
            Ok(packet) => packet,
            Err(error) => {
                mismatches.push(Mismatch {
                    vector_id: vector.vector_id.clone(),
                    detail: format!("oracle refused to derive: {error}"),
                });
                continue;
            }
        };
        mismatches.extend(consequence_mismatches(vector, &packet));

        let derived = String::from_utf8(packet_bytes(&packet)?)?;
        if update_golden {
            let path = golden_path(&vector.vector_id)?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
            let existing = read_golden_normalized(&vector.vector_id)?;
            if existing.as_deref() != Some(derived.as_str()) {
                fs::write(&path, &derived)
                    .with_context(|| format!("writing golden {}", path.display()))?;
                println!("updated golden for {:?}", vector.vector_id);
                regenerated += 1;
            }
        } else {
            match read_golden_normalized(&vector.vector_id)? {
                Some(golden) => {
                    if golden != derived {
                        mismatches.push(Mismatch {
                            vector_id: vector.vector_id.clone(),
                            detail: "derived packet differs from checked-in golden \
                                     (determinism/currentness drift)"
                                .into(),
                        });
                    }
                }
                None => mismatches.push(Mismatch {
                    vector_id: vector.vector_id.clone(),
                    detail: "no checked-in golden; run once with --update-golden as an \
                             explicit writer action"
                        .into(),
                }),
            }
        }
    }

    if !mismatches.is_empty() {
        eprintln!("STANDALONE VECTOR CHECK FAILURES:");
        for mismatch in &mismatches {
            eprintln!("  [{}] {}", mismatch.vector_id, mismatch.detail);
        }
        bail!("standalone-vectors check failed with {} mismatch(es)", mismatches.len());
    }

    if update_golden && regenerated == 0 {
        println!("goldens already current; second generation produced no diff");
    }
    println!(
        "standalone-vectors check passed: {} vectors ({} platform-neutral), \
         independent oracle, no production execution",
        vectors.len(),
        vectors
            .iter()
            .filter(
                |vector| vector.platform_classification == PlatformClassification::PlatformNeutral
            )
            .count(),
    );
    Ok(())
}

/// Renders one vector's full derivation for review.
pub fn run_explain(vector_id: &str) -> Result<()> {
    let vectors = load_corpus()?;
    let vector = vectors.iter().find(|vector| vector.vector_id == vector_id).ok_or_else(|| {
        let known: Vec<&str> = vectors.iter().map(|vector| vector.vector_id.as_str()).collect();
        eyre!("unknown vector {vector_id:?}; known vectors: {known:?}")
    })?;

    let packet = oracle::derive_packet(vector, Deviation::None)
        .map_err(|error| eyre!("derive failed: {error}"))?;

    println!("vector           : {}", vector.vector_id);
    println!("family           : {}", vector.family);
    println!("route/mode       : {:?} / {:?}", packet.route, packet.mode);
    println!("product unit     : {:?}", packet.effective_product_unit);
    println!("subject digest   : {}", shorten(&packet.resolved_subject_digest));
    if let Some(fallback) = &packet.fallback_subject_digest {
        println!("fallback digest  : {}", shorten(fallback));
    }
    println!("executed stages  :");
    for execution in &packet.executed_stages {
        println!(
            "  {:>16} {:28} {:9} {} preds={}",
            execution.attempt_id,
            stage_display(execution.stage_id),
            format!("{:?}", execution.result).to_lowercase(),
            shorten(&execution.receipt_digest),
            execution.predecessor_digests.len(),
        );
    }
    if !packet.skipped_stages.is_empty() {
        println!("skipped stages   :");
        for skip in &packet.skipped_stages {
            println!(
                "  {:>16} {:28} ({})",
                skip.attempt_id,
                stage_display(skip.stage_id),
                skip.authorization
            );
        }
    }
    for observation in &packet.observations {
        println!("observation      : [{}] {}", observation.kind, observation.detail);
    }
    for effect in &packet.effects {
        println!("effect           : {:?} {}", effect.level, effect.kind);
    }
    println!(
        "terminal         : {:?} at {} reason={:?} action={:?}",
        packet.terminal.result,
        stage_display(packet.terminal.stage_id),
        packet.terminal.reason_family,
        packet.terminal.action_class
    );
    println!(
        "ceilings         : effects={:?} claims={:?} pair_claims={}",
        packet.side_effect_ceiling, packet.claim_ceiling, packet.pair_claims_satisfied
    );

    let mismatches = consequence_mismatches(vector, &packet);
    if mismatches.is_empty() {
        println!("assertions       : all authored expectations match the derived packet");
        Ok(())
    } else {
        for mismatch in &mismatches {
            println!("ASSERTION FAILURE: {}", mismatch.detail);
        }
        bail!("explain: {} authored expectation(s) diverge", mismatches.len());
    }
}

fn shorten(digest: &str) -> String {
    digest.chars().take(19).collect()
}

/// Applies every registered mutation to its target vectors and fails unless
/// each application flips its golden. The conformant anchor (no deviations)
/// must reproduce every golden exactly before any mutation runs.
pub fn run_mutation_check() -> Result<()> {
    let vectors = load_corpus()?;
    let mut corpus: BTreeMap<String, Vector> = BTreeMap::new();
    for vector in vectors {
        corpus.insert(vector.vector_id.clone(), vector);
    }

    // Anchor: conformant composition equals every golden.
    for (id, vector) in &corpus {
        let packet = derive_anchor(vector, id)?;
        let derived = String::from_utf8(packet_bytes(&packet)?)?;
        match read_golden_normalized(id)? {
            Some(golden) if golden == derived => {}
            _ => bail!(
                "mutation-check anchor failed: golden for {id} does not match the \
                 conformant derivation"
            ),
        }
    }

    // Bank hygiene: unique ids, non-empty targets, known vectors.
    let mut seen_ids = BTreeSet::new();
    for spec in MUTATION_BANK {
        if !seen_ids.insert(spec.id) {
            bail!("duplicate mutation id {:?}", spec.id);
        }
        if spec.target_vectors.is_empty() {
            bail!("mutation {:?} has no target vectors", spec.id);
        }
        for target in spec.target_vectors {
            if !corpus.contains_key(*target) {
                bail!("mutation {:?} targets unknown vector {:?}", spec.id, target);
            }
        }
    }

    let mut survivors = Vec::new();
    let mut applications = 0usize;
    for spec in MUTATION_BANK {
        for target in spec.target_vectors {
            applications += 1;
            let vector = &corpus[*target];
            let flipped = match oracle::derive_packet(vector, spec.deviation) {
                Err(OracleError::Redaction(message)) => {
                    Some(format!("redaction scanner fired: {message}"))
                }
                // Fix 1 (#13295): a StageGraph error from a mutated derivation
                // is a valid catch signal. The mutation introduced a predecessor-
                // chain inconsistency (e.g. WarnAndContinue bypasses a mandatory
                // stage, leaving a declared successor's predecessor unresolved).
                // A conformant derivation never reaches predecessors_of for that
                // stage because the mandatory failure stops the attempt first;
                // the inconsistency is observable only under the mutation, so
                // the oracle correctly rejects it.
                Err(OracleError::StageGraph(message)) => {
                    Some(format!("stage-graph violation detected: {message}"))
                }
                Err(OracleError::CorpusRule(message)) => {
                    survivors.push(format!(
                        "{} x {}: deviation could not be applied (corpus rule): {message}",
                        spec.id, target
                    ));
                    None
                }
                Ok(packet) => {
                    let derived = String::from_utf8(packet_bytes(&packet)?)?;
                    match read_golden_normalized(target)? {
                        Some(golden) if golden == derived => {
                            survivors.push(format!(
                                "{} x {}: mutation SURVIVED (packet identical to golden)",
                                spec.id, target
                            ));
                            None
                        }
                        Some(_) => Some("packet diverged from golden".to_string()),
                        None => {
                            survivors
                                .push(format!("{} x {}: golden vanished mid-run", spec.id, target));
                            None
                        }
                    }
                }
            };
            if let Some(reason) = flipped {
                println!("  caught  {:<34} {:42} {reason}", spec.id, target);
            }
        }
    }

    println!("mutation bank     :");
    for spec in MUTATION_BANK {
        println!("  {:<34} {}", spec.id, spec.title);
    }

    if !survivors.is_empty() {
        eprintln!("MUTATION SURVIVORS:");
        for entry in &survivors {
            eprintln!("  {entry}");
        }
        bail!("{} mutation application(s) did not flip their golden", survivors.len());
    }

    println!(
        "mutation-check passed: {} mutations, {applications} applications all caught; \
         conformant anchor reproduced every golden",
        MUTATION_BANK.len(),
    );
    Ok(())
}

fn derive_anchor(vector: &Vector, id: &str) -> Result<oracle::SemanticPacket> {
    oracle::derive_packet(vector, Deviation::None)
        .map_err(|error| eyre!("{id}: anchor derivation failed: {error}"))
}
