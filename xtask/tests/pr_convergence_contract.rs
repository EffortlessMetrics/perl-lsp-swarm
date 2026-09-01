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
        std::io::Error::new(
            error.kind(),
            format!("failed to read {}: {error}", full.display()),
        )
        .into()
    })
}

fn neutral(text: &str) -> String {
    text.replace("`$", "`")
}

fn collapsed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn prose(text: &str) -> String {
    text.replace(['`', '*'], "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
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
            let section_start = start.ok_or_else(|| format!("missing section {heading:?}"))?;
            return Ok(&document[section_start..offset]);
        }
        offset += line.len();
    }
    start
        .map(|section_start| &document[section_start..])
        .ok_or_else(|| format!("missing section {heading:?}"))
}

fn table_rows(body: &str) -> Vec<Vec<String>> {
    body.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with('|') {
                return None;
            }
            let cells = trimmed
                .trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().trim_matches('`').to_string())
                .collect::<Vec<_>>();
            if cells.is_empty()
                || cells
                    .iter()
                    .all(|cell| cell.chars().all(|ch| ch == '-' || ch == ':'))
                || matches!(
                    cells.first().map(String::as_str),
                    Some("Disposition" | "Observation" | "Later event")
                )
            {
                return None;
            }
            Some(cells)
        })
        .collect()
}

fn validate_contract(spec: &str) -> Result<(), String> {
    let required_markers = [
        "# PLSP-SPEC-0006: PR semantic incorporation and disposition",
        "Status: accepted (amended 2026-08-11)",
        "Those requirements are superseded by this amendment.",
        "### Semantic candidate and proof",
        "### Integration",
        "### Live required status",
        "### Merge race and landed result",
        "There is no mechanical one-rebase limit.",
        "gh pr merge <n> --squash --match-head-commit <current-head-sha>",
    ];
    for marker in required_markers {
        if !spec.contains(marker) {
            return Err(format!(
                "missing current semantic-convergence marker {marker:?}"
            ));
        }
    }

    if spec.lines().any(|line| line.starts_with("Linked plan:")) {
        return Err("obsolete release-plan authority remains".to_string());
    }

    let disposition_rows = table_rows(section(spec, "## Canonical dispositions")?);
    let actual_dispositions = disposition_rows
        .iter()
        .filter_map(|row| row.first().cloned())
        .collect::<BTreeSet<_>>();
    let expected_dispositions = [
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
    if actual_dispositions != expected_dispositions {
        return Err(format!(
            "canonical disposition set changed: expected {expected_dispositions:?}, got {actual_dispositions:?}"
        ));
    }

    let invalidation_rows = table_rows(section(spec, "## Invalidation matrix")?);
    let unrelated_main = invalidation_rows
        .iter()
        .find(|row| {
            row.first()
                .is_some_and(|cell| prose(cell) == "unrelated main movement")
        })
        .ok_or_else(|| "missing unrelated-main invalidation row".to_string())?;
    if unrelated_main
        .last()
        .is_none_or(|response| !prose(response).contains("leave the candidate unchanged"))
    {
        return Err("unrelated main movement no longer preserves the candidate".to_string());
    }

    let observation_rows = table_rows(section(spec, "## Conflict and unknown-state semantics")?);
    let behind_only = observation_rows
        .iter()
        .find(|row| row.first().is_some_and(|cell| cell == "BEHIND_ONLY"))
        .ok_or_else(|| "missing BEHIND_ONLY observation".to_string())?;
    if behind_only
        .last()
        .is_none_or(|route| !prose(route).contains("no required action"))
    {
        return Err("BEHIND_ONLY must remain a no-action observation".to_string());
    }

    let base_section = prose(section(spec, "## Base reconciliation and rebase")?);
    for required in [
        "a reconcile_base_for_concrete_reason disposition must name at least one reason",
        "an actual textual conflict",
        "current main changed the same semantic contract",
        "an explicit stack prerequisite changed",
        "live branch protection or merge-queue policy requires a current integration basis",
        "selected proof cannot be interpreted without incorporating the prerequisite",
    ] {
        if !base_section.contains(required) {
            return Err(format!("base reconciliation lost prerequisite {required:?}"));
        }
    }
    for insufficient in [
        "the candidate is old or inactive",
        "the branch is many commits behind",
        "unrelated files changed on main",
        "a current status is missing",
        "a prior rebase already happened or has not happened yet",
    ] {
        if !base_section.contains(insufficient) {
            return Err(format!(
                "base reconciliation lost insufficiency rule {insufficient:?}"
            ));
        }
    }

    let normalized = prose(spec);
    for forbidden in [
        "every candidate must update its base before merge",
        "all candidates must rebase before merge",
        "behind branches must be updated before merge",
        "every pull request must rebase onto current main",
        "the branch must be current with main before merge",
    ] {
        if normalized.contains(forbidden) {
            return Err(format!(
                "mandatory-refresh paraphrase restored: {forbidden:?}"
            ));
        }
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
    let admission = repair
        .find("Judge finding validity and current-candidate admission separately.")
        .ok_or("missing admission decision")?;
    let promotion = repair
        .find("Do not treat comments as independent patch instructions.")
        .ok_or("missing promotion boundary")?;
    if admission >= promotion {
        return Err("class promotion precedes candidate admission".into());
    }

    let packet = collapsed(section(&address, "### Return packet")?);
    require_all(
        &packet,
        &[
            "candidate_changed",
            "claim_changed",
            "stale_review_dimensions",
            "earliest still-missing judgment",
        ],
        "return packet",
    )?;

    let procedure = collapsed(section(&address, "## Procedure")?);
    require_all(
        &procedure,
        &[
            "Run a class-level falsifier only when a failure class was promoted.",
            "do not create an empty repair commit when no candidate bytes need to change",
            "When the candidate or claim changed or review dimensions became stale",
            "candidate_changed=false",
            "claim_changed=false",
            "preserve current proof/review conclusions and do not manufacture another challenge cycle",
        ],
        "procedure",
    )?;

    let routes = collapsed(section(&address, "## Routes")?);
    require_all(
        &routes,
        &[
            "`candidate_changed=true`, `claim_changed=true`, or non-empty `stale_review_dimensions`",
            "`candidate_changed=false`, `claim_changed=false`, and empty `stale_review_dimensions`",
            "preserve current proof/review and continue at the earliest still-missing judgment",
        ],
        "routes",
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

    for forbidden in [
        "CLASS_REPAIR_REQUIRED",
        "REPAIR_WAVE_NOT_PROVEN",
        "HEAD_STABILIZED_FOR_CI",
    ] {
        if address.contains(forbidden) || finish.contains(forbidden) {
            return Err(format!("minted overlapping state {forbidden}"));
        }
    }
    if !address.contains("`MUTABLE_FINDINGS_OPEN`")
        || !finish.contains("`MUTABLE_FINDINGS_OPEN`")
    {
        return Err("lost canonical mutable-findings route".into());
    }
    Ok(())
}

#[test]
fn accepted_spec_has_closed_semantic_model() -> Result<(), DynError> {
    let root = root()?;
    let spec = read(&root, "docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md")?;

    validate_contract(&spec).map_err(|error| format!("PLSP-SPEC-0006: {error}"))?;
    Ok(())
}

#[test]
fn ratchet_rejects_paraphrased_mandatory_refresh() -> Result<(), DynError> {
    let root = root()?;
    let spec = read(&root, "docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md")?;
    let mutated = format!("{spec}\n\nEvery candidate must update its base before merge.\n");

    assert!(
        validate_contract(&mutated).is_err(),
        "paraphrased mandatory-base-refresh doctrine must fail the contract"
    );
    Ok(())
}

#[test]
fn ratchet_rejects_age_driven_disposition() -> Result<(), DynError> {
    let root = root()?;
    let spec = read(&root, "docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md")?;
    let anchor = "| `NOT_PROVEN` | Required source, review, proof, policy, or tool evidence could not be established |";
    let replacement = format!("| `NEEDS_REBASE` | The branch is behind `main` |\n{anchor}");
    let mutated = spec.replacen(anchor, &replacement, 1);

    assert_ne!(mutated, spec, "disposition mutation fixture must apply");
    assert!(
        validate_contract(&mutated).is_err(),
        "an added age/behind disposition must fail the closed model"
    );
    Ok(())
}

#[test]
fn ratchet_rejects_behind_only_update_route() -> Result<(), DynError> {
    let root = root()?;
    let spec = read(&root, "docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md")?;
    let current = "| `BEHIND_ONLY` | The candidate is conflict-free while `main` advanced | no required action |";
    let regressed = "| `BEHIND_ONLY` | The candidate is conflict-free while `main` advanced | update the branch before merge |";
    let mutated = spec.replacen(current, regressed, 1);

    assert_ne!(mutated, spec, "BEHIND_ONLY mutation fixture must apply");
    assert!(
        validate_contract(&mutated).is_err(),
        "behind-only branch mutation must fail the contract"
    );
    Ok(())
}

#[test]
fn ratchet_retains_invalidation_and_reconciliation_coverage() -> Result<(), DynError> {
    let root = root()?;
    let spec = read(&root, "docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md")?;

    let invalidation = spec.replacen(
        "leave the candidate unchanged",
        "refresh the candidate unconditionally",
        1,
    );
    assert_ne!(invalidation, spec, "invalidation mutation fixture must apply");
    assert!(
        validate_contract(&invalidation).is_err(),
        "unrelated-main invalidation regression must fail the contract"
    );

    let reconciliation = spec.replacen(
        "an actual textual conflict",
        "a branch that is merely behind",
        1,
    );
    assert_ne!(
        reconciliation, spec,
        "base-reconciliation mutation fixture must apply"
    );
    assert!(
        validate_contract(&reconciliation).is_err(),
        "base-reconciliation prerequisite loss must fail the contract"
    );

    Ok(())
}

#[test]
fn canonical_disposition_parser_ignores_surrounding_prose() {
    let body = "Introductory prose.\n\n| Disposition | Meaning |\n| --- | --- |\n| `BLOCKED` | Wait |\n\nClosing prose.";
    let rows = table_rows(body);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][0], "BLOCKED");
}

#[test]
fn catalogs_name_the_current_contract() -> Result<(), DynError> {
    let root = root()?;
    let catalog = read(&root, "docs/specs/README.md")?;
    let index = read(&root, "docs/INDEX.md")?;

    assert!(
        catalog.contains("PLSP-SPEC-0006: PR semantic incorporation and disposition"),
        "spec catalog must expose the amended title"
    );
    assert!(
        index.contains("PR Semantic Incorporation and Disposition Spec"),
        "documentation index must point to the amended contract"
    );
    assert!(
        !index.contains("0.14.0 Readiness Queue](releases/0.14.0-readiness.md) — current-release"),
        "documentation index must not present the historical 0.14.0 queue as current"
    );

    Ok(())
}

#[test]
fn provider_review_repair_convergence_is_bounded() -> Result<(), DynError> {
    let root = root()?;
    for (provider, prefix) in [("Codex", ".agents"), ("Claude", ".claude")] {
        let address = read(
            &root,
            &format!("{prefix}/skills/address-review-comments/SKILL.md"),
        )?;
        let finish = read(&root, &format!("{prefix}/skills/finish-pr/SKILL.md"))?;
        validate_review_wave(&address, &finish)
            .map_err(|error| format!("{provider}: {error}"))?;

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

        let claim_route = address.replacen(
            "`candidate_changed=true`, `claim_changed=true`, or non-empty `stale_review_dimensions`",
            "`candidate_changed=true` or non-empty `stale_review_dimensions`",
            1,
        );
        assert_ne!(claim_route, address);
        assert!(validate_review_wave(&claim_route, &finish).is_err());

        let claim_procedure = address.replacen(
            "When the candidate or claim changed or review dimensions became stale",
            "When the candidate changed or review dimensions became stale",
            1,
        );
        assert_ne!(claim_procedure, address);
        assert!(validate_review_wave(&claim_procedure, &finish).is_err());

        assert!(
            validate_review_wave(&format!("{address}\nCLASS_REPAIR_REQUIRED\n"), &finish)
                .is_err()
        );
        let thaw = finish.replacen(
            "bounded fresh merge-tree re-evaluation selected by",
            "ordinary behind-only update selected by",
            1,
        );
        assert_ne!(thaw, finish);
        assert!(validate_review_wave(&address, &thaw).is_err());
    }
    Ok(())
}
