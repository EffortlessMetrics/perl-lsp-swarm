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

use std::fs;
use std::path::PathBuf;

use anyhow::{Result, anyhow, bail};

const WORKFLOW: &str = ".github/workflows/ripr-liveness.yml";
const ALREADY_POSTED_DEF: &str = "def already_posted(";
const CHECK_RUNS_INVOCATION_OPEN: &str = "[\"gh\", \"api\",";

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
/// `already_posted` lives inside the inline Python of the
/// `Post advisory check runs` step, so a textual slice is the closest unit we
/// can target without re-parsing the embedded Python. The slice is the single
/// Python list passed to `subprocess.run([...])` whose first element is the
/// string `"gh"` and which targets the check-runs endpoint — that is the only
/// call whose `--paginate`, `filter=all`, `per_page`, and `--jq` properties
/// matter for the dedup. Including the surrounding docstring or the post step
/// body would let the docstring's prose carry the contract and leave a real
/// regression unobserved.
///
/// The same workflow contains other `["gh", "api", ...]` calls (one per run
/// for the snapshot's job count, one per head for fork pulls); the dedup
/// call is the one AFTER `def already_posted(`, so anchor on the def first
/// and take the first `["gh", "api",` that follows.
fn check_runs_invocation(workflow: &str) -> Result<&str> {
    let def_pos = workflow
        .find(ALREADY_POSTED_DEF)
        .ok_or_else(|| anyhow!("{WORKFLOW} has no `def already_posted(`"))?;
    let after_def = &workflow[def_pos..];
    let open_idx = after_def
        .find(CHECK_RUNS_INVOCATION_OPEN)
        .ok_or_else(|| anyhow!("{WORKFLOW} has no `gh api` invocation inside `already_posted`"))?;
    let rest = &after_def[open_idx..];
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
        call.contains("\".check_runs[].external_id\""),
        "the dedup `gh api` call must read `external_id`, not `name` — every \
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
        ("external_id switched to name", "\".check_runs[].external_id\"", "\".check_runs[].name\""),
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
    if !call.contains("\".check_runs[].external_id\"") {
        bail!("the dedup `gh api` call must read `external_id`");
    }
    Ok(())
}

#[test]
fn ripr_liveness_dedup_read_is_paired_with_the_post_step() -> Result<()> {
    // The dedup read only matters while the post step that consumes its verdict
    // is still wired up. A future refactor that moves `already_posted` into a
    // helper module without keeping the post step would silently lose the
    // contract, so pin both halves of the loop together.
    let workflow = read_workflow()?;

    assert!(
        workflow.contains("- name: Post advisory check runs"),
        "{WORKFLOW} has no `Post advisory check runs` step; the dedup read has \
         no consumer to guard"
    );
    assert!(
        workflow.contains(ALREADY_POSTED_DEF),
        "{WORKFLOW} no longer carries `already_posted`; the dedup contract has \
         no subject"
    );

    Ok(())
}

/// #16133: the workflow must read `external_id`, not `name`.
///
/// Every post under this workflow carries the same name (`ripr+ liveness`),
/// and the same name can carry more than one id as the predecessor starts
/// and finishes. The dedup verdict is therefore `external_id in <seen>` and
/// the selector must be `.check_runs[].external_id`. Reading `.name` would
/// collapse every id into the single one and re-post on every flip.
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

    Ok(())
}
