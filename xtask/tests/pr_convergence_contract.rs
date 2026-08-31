//! Regression contract for PLSP-SPEC-0006 and review-wave convergence.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

type DynError = Box<dyn std::error::Error>;

fn root() -> Result<PathBuf, DynError> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

fn read(root: &Path, path: &str) -> Result<String, DynError> {
    let full = root.join(path);
    fs::read_to_string(&full).map_err(|error| {
        std::io::Error::new(error.kind(), format!("failed to read {}: {error}", full.display())).into()
    })
}

fn neutral(text: &str) -> String {
    text.replace("`$", "`")
}

fn collapsed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn heading_level(line: &str) -> Option<usize> {
    let level = line.bytes().take_while(|byte| *byte == b'#').count();
    (level > 0 && line.as_bytes().get(level) == Some(&b' ')).then_some(level)
}

fn section<'a>(document: &'a str, heading: &str) -> Result<&'a str, String> {
    let level = heading_level(heading).ok_or_else(|| format!("invalid heading {heading:?}"))?;
    let matches = document.lines().filter(|line| *line == heading).count();
    if matches != 1 {
        return Err(format!("heading {heading:?} occurs {matches} times"));
    }

    let mut offset = 0;
    let mut start = None;
    for line in document.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        let body = body.strip_suffix('\r').unwrap_or(body);
        if start.is_none() {
            if body == heading {
                start = Some(offset + line.len());
            }
        } else if heading_level(body).is_some_and(|candidate| candidate <= level) {
            return Ok(&document[start.expect("section start exists")..offset]);
        }
        offset += line.len();
    }
    start.map(|start| &document[start..]).ok_or_else(|| format!("missing section {heading:?}"))
}

fn table_first_cells(body: &str) -> BTreeSet<String> {
    body.lines()
        .filter_map(|line| {
            let cells = line.trim().trim_matches('|').split('|').map(str::trim).collect::<Vec<_>>();
            let first = cells.first()?.trim_matches('`');
            (!first.is_empty() && first != "Disposition" && !first.chars().all(|ch| ch == '-' || ch == ':'))
                .then(|| first.to_string())
        })
        .collect()
}

fn validate_spec(spec: &str) -> Result<(), String> {
    for marker in [
        "# PLSP-SPEC-0006: PR semantic incorporation and disposition",
        "Status: accepted (amended 2026-08-11)",
        "Those requirements are superseded by this amendment.",
        "There is no mechanical one-rebase limit.",
        "gh pr merge <n> --squash --match-head-commit <current-head-sha>",
    ] {
        if !spec.contains(marker) {
            return Err(format!("missing spec marker {marker:?}"));
        }
    }
    if spec.lines().any(|line| line.starts_with("Linked plan:")) {
        return Err("obsolete linked-plan authority remains".into());
    }

    let expected = [
        "MERGE_EXISTING_CANDIDATE",
        "REPAIR_EXISTING_CANDIDATE",
        "RESOLVE_CONFLICT",
        "REVIEW_INTEGRATION_INTERACTION",
        "RECONCILE_BASE_FOR_CONCRETE_REASON",
        "SALVAGE_UNIQUE_DELTA",
        "SUPERSEDED_WITH_EVIDENCE",
        "NOT_PROVEN",
        "BLOCKED",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<BTreeSet<_>>();
    let actual = table_first_cells(section(spec, "## Canonical dispositions")?);
    if actual != expected {
        return Err(format!("canonical dispositions changed: {actual:?}"));
    }

    let normalized = collapsed(spec).to_ascii_lowercase();
    for forbidden in [
        "every candidate must update its base before merge",
        "all candidates must rebase before merge",
        "behind branches must be updated before merge",
        "every pull request must rebase onto current main",
    ] {
        if normalized.contains(forbidden) {
            return Err(format!("mandatory refresh restored: {forbidden}"));
        }
    }

    let conflict = collapsed(section(spec, "## Conflict and unknown-state semantics")?);
    if !conflict.contains("`BEHIND_ONLY`") || !conflict.contains("no required action") {
        return Err("BEHIND_ONLY must remain no-action".into());
    }
    Ok(())
}

fn require_all(body: &str, markers: &[&str], label: &str) -> Result<(), String> {
    for marker in markers {
        if !body.contains(marker) {
            return Err(format!("{label} lost {marker:?}"));
        }
    }
    Ok(())
}

fn validate_review_wave(address: &str, finish: &str) -> Result<(), String> {
    let address = neutral(address);
    let finish = neutral(finish);
    let repair = collapsed(section(&address, "### Repair-wave boundary")?);
    require_all(
        &repair,
        &[
            "Treat a review as usefully in flight only when GitHub exposes a durable current-subject signal",
            "A prior bot comment, stale review, reaction, quota warning, or expectation that a reviewer usually runs is not liveness.",
            "materially false, misleading, unsafe, under-proven, incompatible with its accepted contract, or outside its stated risk/rollback boundary",
            "repair remains inside the same acceptance-and-rollback proposition",
            "separately reversible proposition, consumer, authority, proof system, release horizon, or rollback seam",
            "Only when two or more findings share the same underlying mechanism, or one repair exposes another instance of that mechanism",
            "promote them to one failure class",
            "one-use or non-gated proof instrument",
            "run `simplify-candidate` before final challenge",
        ],
        "repair boundary",
    )?;
    let admission = repair.find("Judge finding validity and current-candidate admission separately.")
        .ok_or("missing admission decision")?;
    let promotion = repair.find("Do not treat comments as independent patch instructions.")
        .ok_or("missing promotion boundary")?;
    if admission >= promotion {
        return Err("class promotion precedes candidate admission".into());
    }

    let packet = collapsed(section(&address, "### Return packet")?);
    require_all(&packet, &["candidate_changed", "claim_changed", "stale_review_dimensions", "earliest still-missing judgment"], "return packet")?;

    let procedure = collapsed(section(&address, "## Procedure")?);
    require_all(
        &procedure,
        &[
            "Run a class-level falsifier only when a failure class was promoted.",
            "do not create an empty repair commit when no candidate bytes need to change",
            "candidate_changed=false",
            "claim_changed=false",
            "preserve current proof/review conclusions and do not manufacture another challenge cycle",
        ],
        "procedure",
    )?;

    let stabilization = collapsed(section(&finish, "## Repair waves and head stabilization")?);
    require_all(
        &stabilization,
        &[
            "Do not publish one commit per comment.",
            "when a class was promoted",
            "candidate_changed",
            "claim_changed",
            "stale_review_dimensions",
            "do not create an empty repair commit or manufacture another final-challenge cycle",
            "run `simplify-candidate` before final challenge",
            "bounded fresh merge-tree re-evaluation selected by `verify-live-ci`",
            "sole action owner for that exception",
        ],
        "finish-pr",
    )?;

    for forbidden in ["CLASS_REPAIR_REQUIRED", "REPAIR_WAVE_NOT_PROVEN", "HEAD_STABILIZED_FOR_CI"] {
        if address.contains(forbidden) || finish.contains(forbidden) {
            return Err(format!("minted overlapping state {forbidden}"));
        }
    }
    if !address.contains("`MUTABLE_FINDINGS_OPEN`") || !finish.contains("`MUTABLE_FINDINGS_OPEN`") {
        return Err("lost canonical mutable-findings route".into());
    }
    Ok(())
}

#[test]
fn accepted_spec_has_closed_semantic_model() -> Result<(), DynError> {
    validate_spec(&read(&root()?, "docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md")?)
        .map_err(|error| format!("PLSP-SPEC-0006: {error}"))?;
    Ok(())
}

#[test]
fn spec_ratchets_reject_refresh_and_disposition_regressions() -> Result<(), DynError> {
    let spec = read(&root()?, "docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md")?;
    assert!(validate_spec(&format!("{spec}\nEvery candidate must update its base before merge.\n")).is_err());
    let anchor = "| `NOT_PROVEN` | Required source, review, proof, policy, or tool evidence could not be established |";
    let mutated = spec.replacen(anchor, &format!("| `NEEDS_REBASE` | The branch is behind `main` |\n{anchor}"), 1);
    assert_ne!(mutated, spec);
    assert!(validate_spec(&mutated).is_err());
    Ok(())
}

#[test]
fn catalogs_name_the_current_contract() -> Result<(), DynError> {
    let root = root()?;
    assert!(read(&root, "docs/specs/README.md")?.contains("PLSP-SPEC-0006: PR semantic incorporation and disposition"));
    assert!(read(&root, "docs/INDEX.md")?.contains("PR Semantic Incorporation and Disposition Spec"));
    Ok(())
}

#[test]
fn provider_review_repair_convergence_is_bounded() -> Result<(), DynError> {
    let root = root()?;
    for (provider, prefix) in [("Codex", ".agents"), ("Claude", ".claude")] {
        let address = read(&root, &format!("{prefix}/skills/address-review-comments/SKILL.md"))?;
        let finish = read(&root, &format!("{prefix}/skills/finish-pr/SKILL.md"))?;
        validate_review_wave(&address, &finish).map_err(|error| format!("{provider}: {error}"))?;

        let narrow = address.replacen(
            "materially false, misleading, unsafe, under-proven, incompatible with its accepted\ncontract, or outside its stated risk/rollback boundary",
            "make the present feature sentence literally false",
            1,
        );
        assert_ne!(narrow, address);
        assert!(validate_review_wave(&narrow, &finish).is_err());

        let isolated = address.replacen(
            "Only when two or more findings\nshare the same underlying mechanism, or one repair exposes another instance of that\nmechanism, promote them to one failure class.",
            "For every isolated finding, promote it to one failure class.",
            1,
        );
        assert_ne!(isolated, address);
        assert!(validate_review_wave(&isolated, &finish).is_err());

        let admission = "A confirmed finding\nbelongs in this candidate when leaving it unresolved would make the candidate\nmaterially false, misleading, unsafe, under-proven, incompatible with its accepted\ncontract, or outside its stated risk/rollback boundary, and the repair remains inside\nthe same acceptance-and-rollback proposition.";
        let moved = address.replacen(admission, "", 1).replacen(
            "### Return packet",
            &format!("### Return packet\n\n{admission}"),
            1,
        );
        assert_ne!(moved, address);
        assert!(validate_review_wave(&moved, &finish).is_err());

        let duplicate = format!("{address}\n\n### Repair-wave boundary\n\nweaker duplicate\n");
        assert!(validate_review_wave(&duplicate, &finish).is_err());

        let replay = address.replacen(
            "preserve current proof/review conclusions and do not manufacture another challenge cycle",
            "rerun all proof and review after every disposition",
            1,
        );
        assert_ne!(replay, address);
        assert!(validate_review_wave(&replay, &finish).is_err());

        let guessed = address.replacen(
            "A prior bot comment, stale review, reaction, quota\nwarning, or expectation that a reviewer usually runs is not liveness.",
            "A prior bot comment or reaction proves review is in flight.",
            1,
        );
        assert_ne!(guessed, address);
        assert!(validate_review_wave(&guessed, &finish).is_err());

        assert!(validate_review_wave(&format!("{address}\nCLASS_REPAIR_REQUIRED\n"), &finish).is_err());
        let thaw = finish.replacen("bounded fresh merge-tree re-evaluation selected by", "ordinary behind-only update selected by", 1);
        assert_ne!(thaw, finish);
        assert!(validate_review_wave(&address, &thaw).is_err());
    }
    Ok(())
}
