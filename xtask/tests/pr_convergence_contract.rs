//! Regression contract for PLSP-SPEC-0006, review waves, and status production.

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
        std::io::Error::new(error.kind(), format!("failed to read {}: {error}", full.display()))
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
    // A fenced line such as `# gh pr merge ...` is example text, not a
    // heading, and must never terminate the scanned section.
    let mut in_fence = false;
    for line in document.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        let body = body.strip_suffix('\r').unwrap_or(body);
        if body.trim_start().starts_with("```") {
            in_fence = !in_fence;
            offset += line.len();
            continue;
        }
        if in_fence {
            offset += line.len();
            continue;
        }
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
                || cells.iter().all(|cell| cell.chars().all(|ch| ch == '-' || ch == ':'))
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
            return Err(format!("missing current semantic-convergence marker {marker:?}"));
        }
    }

    if spec.lines().any(|line| line.starts_with("Linked plan:")) {
        return Err("obsolete release-plan authority remains".to_string());
    }

    let disposition_rows = table_rows(section(spec, "## Canonical dispositions")?);
    let actual_dispositions =
        disposition_rows.iter().filter_map(|row| row.first().cloned()).collect::<BTreeSet<_>>();
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
        .find(|row| row.first().is_some_and(|cell| prose(cell) == "unrelated main movement"))
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
    if behind_only.last().is_none_or(|route| !prose(route).contains("no required action")) {
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
            return Err(format!("base reconciliation lost insufficiency rule {insufficient:?}"));
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
            return Err(format!("mandatory-refresh paraphrase restored: {forbidden:?}"));
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
            "Queue reformation supplies new G evidence",
            "it does not authorize an empty commit or ordinary CI trigger",
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

const STATUS_ACTIONS: [&str; 5] = [
    "Read exact H, current substantive review",
    "If native custody already exists for that H",
    "Observe actual G/base/complete constituents",
    "A concrete transient failure may justify",
    "Read protected native landing and M",
];

fn unique_position(body: &str, marker: &str) -> Result<usize, String> {
    let mut matches = body.match_indices(marker).map(|(position, _)| position);
    let first = matches.next().ok_or_else(|| format!("missing ordered action {marker:?}"))?;
    if matches.next().is_some() {
        return Err(format!("ordered action {marker:?} occurs more than once"));
    }
    Ok(first)
}

fn ordered_positions(body: &str, markers: &[&str]) -> Result<Vec<usize>, String> {
    markers.iter().map(|marker| unique_position(body, marker)).collect()
}

fn yaml_frontmatter(document: &str) -> Result<&str, String> {
    let body = document.strip_prefix("---\n").ok_or("missing opening YAML front matter")?;
    let end = body.find("\n---\n").ok_or("missing closing YAML front matter")?;
    Ok(&body[..end])
}

fn validate_claude_internal_skill(document: &str, label: &str) -> Result<(), String> {
    let frontmatter = yaml_frontmatter(document)?;
    let internal = frontmatter.lines().filter(|line| *line == "user-invocable: false").count();
    if internal != 1 {
        return Err(format!("{label} must contain exactly one user-invocable: false field"));
    }
    if frontmatter.lines().any(|line| line.starts_with("argument-hint:")) {
        return Err(format!("{label} exposes an argument hint for an internal skill"));
    }
    Ok(())
}

fn validate_status_production(triage: &str, verify: &str) -> Result<(), String> {
    let triage = neutral(triage);
    let verify = neutral(verify);
    let packet = collapsed(section(&triage, "## Classification packet")?);
    require_all(
        &packet,
        &[
            "evaluated_subject",
            "required_subject",
            "same_run_sufficient: true | false",
            "status_production_gap: fresh_integration_subject | missing_context | none",
            "This is classification consumed by `verify-live-ci`; it is not a lifecycle result and is not terminal `NOT_PROVEN` while an admissible status-production action may still exist.",
            "`verify-live-ci` alone selects, performs, or routes the next status-production action",
        ],
        "triage classification packet",
    )?;
    for forbidden in ["dispatch a workflow", "request a rerun", "push an empty commit"] {
        if packet.contains(forbidden) {
            return Err(format!("triage owns remote action {forbidden:?}"));
        }
    }

    let order = collapsed(section(&verify, "## Required-status production order")?);
    let positions = ordered_positions(&order, &STATUS_ACTIONS)?;
    if !positions.windows(2).all(|window| window[0] < window[1]) {
        return Err("status-production action precedence changed".into());
    }
    require_all(
        &order,
        &[
            "conditional landing authorization",
            "complete effective protection union",
            "observe it rather than enqueue again",
            "invoke the qualified exact-H enqueue owner once and read back custody",
            "Lost acknowledgment is observation/recovery, never a blind second mutation",
            "every selected named/App-bound result in its exact workflow run and attempt",
            "Neither an associated PR nor a live queue prefix proves complete membership",
            "only while the subject, attempt and eligibility remain valid",
            "Never substitute a pre-queue workflow dispatch or an empty push",
            "A command invocation or API 2xx is not an observed transition",
            "only for exact persisted native custody/run state with an accountable owner and wake event",
            "A supported H publisher is admission evidence; it cannot replace actual G proof",
            "do not issue a second direct merge",
        ],
        "native queue admission and qualification",
    )?;
    let currentness = collapsed(section(&verify, "## Currentness and native group reformation")?);
    require_all(
        &currentness,
        &[
            "Preserve unchanged semantic review and local proof",
            "G and invalidates old G execution evidence",
            "retain `NOT_PROVEN`",
        ],
        "subject currentness",
    )?;

    for forbidden in [
        "FRESH_INTEGRATION_SUBJECT_REQUIRED",
        "GENERATED_PROJECTION_PREDECESSOR_PENDING",
        "Shared generated-projection landing cohorts",
        "Generated-projection predecessor",
    ] {
        if triage.contains(forbidden) || verify.contains(forbidden) {
            return Err(format!("status-production claim retained foreign surface {forbidden}"));
        }
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

    let invalidation =
        spec.replacen("leave the candidate unchanged", "refresh the candidate unconditionally", 1);
    assert_ne!(invalidation, spec, "invalidation mutation fixture must apply");
    assert!(
        validate_contract(&invalidation).is_err(),
        "unrelated-main invalidation regression must fail the contract"
    );

    let reconciliation =
        spec.replacen("an actual textual conflict", "a branch that is merely behind", 1);
    assert_ne!(reconciliation, spec, "base-reconciliation mutation fixture must apply");
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
            validate_review_wave(&format!("{address}\nCLASS_REPAIR_REQUIRED\n"), &finish).is_err()
        );
        let thaw = finish.replacen(
            "Queue reformation\nsupplies new G evidence",
            "Behind-only movement authorizes an ordinary CI trigger",
            1,
        );
        assert_ne!(thaw, finish);
        assert!(validate_review_wave(&address, &thaw).is_err());
    }
    Ok(())
}

#[test]
fn claude_status_production_skills_remain_internal() -> Result<(), DynError> {
    let root = root()?;
    for (label, path) in [
        ("Claude ci-failure-triage", ".claude/skills/ci-failure-triage/SKILL.md"),
        ("Claude verify-live-ci", ".claude/skills/verify-live-ci/SKILL.md"),
    ] {
        let skill = read(&root, path)?;
        validate_claude_internal_skill(&skill, label)
            .map_err(|error| format!("{label}: {error}"))?;

        let exposed = skill.replacen("user-invocable: false\n", "", 1);
        assert_ne!(exposed, skill);
        assert!(validate_claude_internal_skill(&exposed, label).is_err());
    }

    let verify = read(&root, ".claude/skills/verify-live-ci/SKILL.md")?;
    let hinted = verify.replacen(
        "user-invocable: false\n",
        "user-invocable: false\nargument-hint: \"[PR number]\"\n",
        1,
    );
    assert_ne!(hinted, verify);
    assert!(validate_claude_internal_skill(&hinted, "Claude verify-live-ci").is_err());
    Ok(())
}

#[test]
fn provider_status_production_is_single_sourced_and_persistent() -> Result<(), DynError> {
    let root = root()?;
    for (provider, prefix) in [("Codex", ".agents"), ("Claude", ".claude")] {
        let triage = read(&root, &format!("{prefix}/skills/ci-failure-triage/SKILL.md"))?;
        let verify = read(&root, &format!("{prefix}/skills/verify-live-ci/SKILL.md"))?;
        validate_status_production(&triage, &verify)
            .map_err(|error| format!("{provider}: {error}"))?;

        let terminal_triage =
            triage.replacen("is not terminal `NOT_PROVEN`", "is terminal `NOT_PROVEN`", 1);
        assert_ne!(terminal_triage, triage);
        assert!(validate_status_production(&terminal_triage, &verify).is_err());

        for (required, regression) in [
            ("Read exact H, current substantive review", "Enqueue before reviewing H"),
            ("observe it rather than enqueue again", "enqueue it again"),
            ("read back custody", "accept command success as custody"),
            ("never a blind second mutation", "retry the mutation blindly"),
            ("Observe actual G/base/complete constituents", "Observe the first associated PR"),
            ("named/App-bound", "unbound"),
            ("exact workflow run and attempt", "latest green workflow"),
            ("attempt and eligibility remain valid", "attempt and eligibility no longer matter"),
            ("API 2xx is not an observed transition", "API 2xx proves the transition"),
            ("cannot replace actual G proof", "replaces actual G proof"),
            ("do not issue a second direct merge", "issue another direct merge"),
            ("invalidates old G execution evidence", "reuses old G execution evidence"),
            (
                "Never substitute a pre-queue workflow dispatch or an empty push",
                "Push an empty commit to recover CI",
            ),
        ] {
            let broken = verify.replacen(required, regression, 1);
            assert_ne!(broken, verify, "mutation must apply: {required}");
            assert!(
                validate_status_production(&triage, &broken).is_err(),
                "accepted: {regression}"
            );
        }
        let duplicate_action = verify.replacen(
            "## Currentness and native group reformation",
            "Read exact H, current substantive review again.\n\n## Currentness and native group reformation", 1);
        assert_ne!(duplicate_action, verify);
        assert!(validate_status_production(&triage, &duplicate_action).is_err());
        let reversed = verify
            .replace(STATUS_ACTIONS[0], "TEMP_ACTION_MARKER")
            .replace(STATUS_ACTIONS[1], STATUS_ACTIONS[0])
            .replace("TEMP_ACTION_MARKER", STATUS_ACTIONS[1]);
        assert!(validate_status_production(&triage, &reversed).is_err());
        let minted = format!("{verify}\nFRESH_INTEGRATION_SUBJECT_REQUIRED\n");
        assert!(validate_status_production(&triage, &minted).is_err());
    }
    Ok(())
}
