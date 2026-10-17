//! Regression contracts for the ripr-liveness dedup read (#16133).
//!
//! `.github/workflows/ripr-liveness.yml` deduplicates its advisory check runs by
//! reading the check runs already on a head and looking for its own
//! `external_id`. That read must be paginated across every page of check runs
//! the head ever carried and must scope over every check (filter=all), because
//! every post here carries the single name `ripr+ liveness` and the default
//! `filter=latest` answers "is this the newest liveness id" when the question
//! is "has this id been posted at all". The cron fires every 15 minutes and a
//! stall lasts as long as it lasts, so dropping `--paginate` or `filter=all`
//! re-posts an identical check run for every cron tick past page one.
//!
//! Asserting on the YAML text keeps the contract close to the load-bearing
//! surface (inline Python inside a `run:` block), and it ships a set of
//! negative controls so a regression that drops any one flag or reroutes the
//! `--jq` selector fires fail-closed in this binary rather than silently in
//! production.
//!
//! Enforcement lives on the `Compile All Targets (bit-rot guard)` lane because
//! no other lane ran this binary before; the issue notes that adding the
//! contract here rather than paying the full `ripr+ New Gap Gate` cycle for it
//! was the deliberate trade-off, and the gate row that ultimately executes
//! this binary is the same one that runs the other issue_NNNN contract tests.
//!
//! #16567: where the read lives, and what it selects
//! ------------------------------------------------
//!
//! The read used to sit in the `Post advisory check runs` step, inside a local
//! `already_posted` helper that answered "has this id been posted". Two changes
//! moved it, and this file is re-anchored rather than rewritten:
//!
//! * It now runs in the `Snapshot ripr runs` step and writes
//!   `target/ripr/liveness/posted.json`, because the reporter's memory has to
//!   reach `classify_snapshot` -- the step that runs *before* the post loop --
//!   for a stall that cleared to be withdrawn rather than contradicted.
//! * It selects `{external_id, id}` rather than a bare `external_id`, because
//!   withdrawing a stall means *updating* the check run that carries the red,
//!   and the check-runs API addresses a write by that run's numeric id. A
//!   membership answer is enough to suppress a duplicate and useless for a
//!   retraction.
//!
//! What did **not** change is the property this file exists to hold: the read is
//! still paginated, still scoped over every check, still keyed on
//! `external_id`, and still must not be keyed on `name`. The decision that
//! consumes the read now lives in `scripts/ci/ripr_liveness.py`, where
//! `scripts/ci/test_ripr_liveness.py` proves it; this file keeps guarding the
//! transport flags that no other test can see.

use std::fs;
use std::path::PathBuf;

use anyhow::{Result, anyhow, bail};

const WORKFLOW: &str = ".github/workflows/ripr-liveness.yml";
const CHECK_RUNS_URL: &str = "check-runs?";
const GH_API_INVOCATION_OPEN: &str = "[\"gh\", \"api\",";

fn project_root() -> Result<PathBuf> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow!("CARGO_MANIFEST_DIR has no parent"))?
        .to_path_buf())
}

fn read_workflow() -> Result<String> {
    let root = project_root()?;
    let raw = fs::read_to_string(root.join(WORKFLOW))
        .map_err(|err| anyhow!("reading {WORKFLOW}: {err}"))?;
    // CRLF would make the literal anchors below miss; normalise to LF. The
    // production reads also feed the YAML round-trip and benefit from a single
    // line ending.
    Ok(raw.replace("\r\n", "\n"))
}

/// The single `gh api` Python-list literal that performs the check-runs read.
///
/// That call lives inside the inline Python of the `Snapshot ripr runs` step,
/// so a textual slice is the closest unit we can target without re-parsing the
/// embedded Python. The slice is the single Python list passed to
/// `subprocess.run([...])` that targets the check-runs endpoint.
///
/// The same workflow contains other `["gh", "api", ...]` calls -- one per run
/// for the snapshot's job count, one per in-flight run for its step detail, one
/// per run for fork-pull identity -- and none of them mentions `check-runs`, so
/// the URL fragment is the discriminator. Anchoring on a step name or a helper
/// name instead would re-break the day either is renamed, which is the wrong
/// moment for a guard on the check-runs API to go quiet.
fn check_runs_invocation(workflow: &str) -> Result<&str> {
    let url_pos = workflow
        .find(CHECK_RUNS_URL)
        .ok_or_else(|| anyhow!("{WORKFLOW} has no check-runs read"))?;
    // Walk back to the start of the list literal carrying the URL, so the slice
    // is the whole call and not whatever precedes it in the step.
    let open_idx = workflow[..url_pos]
        .rfind(GH_API_INVOCATION_OPEN)
        .ok_or_else(|| anyhow!("{WORKFLOW} check-runs read is not a `gh api` list literal"))?;
    let rest = &workflow[open_idx..];
    let close_idx = rest
        .find("],\n")
        .ok_or_else(|| anyhow!("{WORKFLOW} `gh api` invocation is not closed by `],`"))?;
    Ok(&rest[..=close_idx + 1])
}

#[test]
fn ripr_liveness_dedup_read_uses_paginate_and_external_id() -> Result<()> {
    let workflow = read_workflow()?;
    let call = check_runs_invocation(&workflow)?;

    // The four properties the dedup read needs to exist.
    assert!(
        call.contains("--paginate"),
        "the dedup `gh api` call must pass `--paginate` so every page of check \
         runs on this head contributes to the seen-id set; without it the cron \
         re-posts an identical check run for every tick past page one. \
         Call:\n{call}"
    );
    assert!(
        call.contains("check-runs?filter=all"),
        "the dedup `gh api` call must request `filter=all` so the dedup sees \
         every check run on the head rather than only the most recent per \
         name — every post here carries the single name `ripr+ liveness`, so \
         the default `filter=latest` answers \"is this the newest liveness id\" \
         when the question is \"has this id been posted at all\". \
         Call:\n{call}"
    );
    assert!(
        call.contains("per_page="),
        "the dedup `gh api` call must set `per_page` on the check-runs URL; \
         the default is 30, and this repository's heads carry 83 check-run \
         rows on the busiest measured case. Call:\n{call}"
    );
    assert!(
        call.contains("external_id"),
        "the dedup `gh api` call must read `external_id`, not `name` - every \
         post here carries the single name `ripr+ liveness` and the same name \
         can carry more than one id as the predecessor starts and finishes. \
         Call:\n{call}"
    );

    Ok(())
}

#[test]
fn ripr_liveness_dedup_read_rejects_each_load_bearing_regression() -> Result<()> {
    let workflow = read_workflow()?;
    let call = check_runs_invocation(&workflow)?;

    // Each mutation strips exactly one of the four load-bearing properties.
    // Keeping them as separate mutations makes a failing test name a precise
    // pointer at the regressed property rather than at "the contract broke".
    let mutations: &[(&str, &str, &str)] = &[
        ("dropped --paginate", "[\"gh\", \"api\", \"--paginate\",", "[\"gh\", \"api\","),
        ("filter=all dropped", "check-runs?filter=all&per_page=100", "check-runs?per_page=100"),
        ("per_page dropped", "check-runs?filter=all&per_page=100", "check-runs?filter=all"),
        (
            "external_id switched to name",
            "select(.external_id != null) | {external_id, id}",
            "select(.name != null) | {name, id}",
        ),
    ];

    for (name, needle, replacement) in mutations {
        assert!(
            call.contains(needle),
            "{name}: the dedup call does not contain the literal `{needle}`; the \
             mutation fixture is stale. Call:\n{call}"
        );
        let mutated = call.replace(needle, replacement);
        assert!(mutated != *call, "{name}: mutation did not change the dedup call");
        assert!(
            dedup_call_holds_the_contract(&mutated).is_err(),
            "{name}: the regressed dedup call was accepted"
        );
    }

    Ok(())
}

fn dedup_call_holds_the_contract(call: &str) -> Result<()> {
    if !call.contains("--paginate") {
        bail!("the dedup `gh api` call must pass `--paginate`");
    }
    if !call.contains("check-runs?filter=all") {
        bail!("the dedup `gh api` call must request `filter=all`");
    }
    if !call.contains("per_page=") {
        bail!("the dedup `gh api` call must set `per_page`");
    }
    if !call.contains("external_id") {
        bail!("the dedup `gh api` call must read `external_id`");
    }
    if call.contains("select(.name != null)") || call.contains("{name, id}") {
        bail!("the dedup `gh api` call must not key the identity on `name`");
    }
    Ok(())
}

#[test]
fn ripr_liveness_dedup_read_is_paired_with_its_consumer() -> Result<()> {
    // The read only matters while something consumes the map it produces. It
    // used to feed `already_posted` inside the post step; it now feeds
    // `classify_snapshot`, which is why the map has to be written to a file and
    // handed to the classify step explicitly. A refactor that moved the read
    // without moving its consumer would silently lose both the dedup and the
    // retraction that depends on it, so pin all three halves together: the
    // read, the file that carries it forward, and the step that consumes it.
    let workflow = read_workflow()?;

    assert!(
        workflow.contains("- name: Post advisory check runs"),
        "{WORKFLOW} has no `Post advisory check runs` step; the dedup read has \
         no consumer to guard"
    );
    assert!(
        workflow.contains("target/ripr/liveness/posted.json"),
        "{WORKFLOW} never writes `posted.json`; the read has nowhere to put the \
         map its consumer needs"
    );
    assert!(
        workflow.contains("--posted target/ripr/liveness/posted.json"),
        "{WORKFLOW} never passes `posted.json` to the classifier; the reporter's \
         memory across cron fires is read and then discarded"
    );
    assert!(
        workflow.contains("posted_checks_from_api"),
        "{WORKFLOW} no longer hands the read to the tested module; a parsing \
         decision has moved back into untested inline Python"
    );

    Ok(())
}

/// #16133: the workflow must read `external_id`, not `name`.
///
/// Every post under this workflow carries the same name (`ripr+ liveness`),
/// and the same name can carry more than one id as the predecessor starts
/// and finishes. The dedup verdict is therefore `external_id in <seen>` and
/// the selector must carry `external_id`. Reading `name` would collapse every
/// id into the single one and re-post on every flip.
#[test]
fn ripr_liveness_dedup_read_selects_external_id_not_name() -> Result<()> {
    let workflow = read_workflow()?;
    let call = check_runs_invocation(&workflow)?;

    assert!(
        !call.contains(".check_runs[].name"),
        "the dedup `gh api` call must NOT select `name`; every post here \
         carries the single name `ripr+ liveness` and reading `name` would \
         collapse every id into the one name and re-post on every flip. \
         Call:\n{call}"
    );
    assert!(
        !call.contains("select(.name != null)"),
        "the dedup `gh api` call must not select rows by `name`; the reporter's \
         own `external_id` is the identity. Call:\n{call}"
    );
    assert!(
        !call.contains("{name, id}"),
        "the dedup `gh api` call must not project `name` into the parsed row; a \
         retraction is addressed by the numeric `id` and keyed to the \
         `external_id`. Call:\n{call}"
    );

    Ok(())
}

/// #16567: the numeric `id` has to survive the read.
///
/// A retraction is a write against one specific check run, addressed by that
/// run's numeric id. A selector returning `external_id` alone would still dedup
/// correctly -- so this regression would pass every assertion above -- and
/// would leave the reporter unable to withdraw anything it ever wrote.
#[test]
fn ripr_liveness_dedup_read_selects_the_numeric_id_too() -> Result<()> {
    let workflow = read_workflow()?;
    let call = check_runs_invocation(&workflow)?;

    assert!(
        call.contains("| @json"),
        "the dedup `gh api` call must emit one JSON object per line under \
         `--paginate`, not one document for the whole page; the reader parses a \
         row per line. Call:\n{call}"
    );
    assert!(
        call.contains("{external_id, id}"),
        "the dedup `gh api` call must project both the identity and the numeric \
         id; a retraction is addressed by the id. Call:\n{call}"
    );

    Ok(())
}
