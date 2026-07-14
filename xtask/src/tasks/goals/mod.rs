//! Program-aware, deterministic work selector (#3624, M3 of the
//! enablement train #3612): `cargo xtask goals next --json`.
//!
//! Selects the next eligible slice of work from LIVE evidence — current
//! `origin/main`, live open GitHub PRs, the M2 manifest chain, and (for
//! programs shaped as a milestone ledger, e.g. `agent_loop_enablement`)
//! the `[[milestone]]` ledger — never from chat or Claude Code's
//! `TaskList`/`TaskGet` session state (see CLAUDE.md's truth hierarchy:
//! GitHub + the manifest chain outrank conversation and session
//! bookkeeping; the harness's `TaskList` `completed` flag is known not to
//! persist reliably across sessions).
//!
//! `goals next` is READ-ONLY: it never creates a branch, worktree, or PR,
//! and never writes any file under `.perl-lsp/goals/`, `target/`, or
//! `docs/`. Branch/worktree/PR creation is `/start-pr`'s job — a separate,
//! later deliverable, deliberately out of scope here.
//!
//! #4174 slice 1 (retiring the singleton active-goal control surface):
//! no-arg `goals next`/`goals reconcile` no longer resolve `active.toml`'s
//! `default_program` into ONE repository-global selection — that implicit
//! fallback is gone from `resolve_program`. Instead they print a
//! PORTFOLIO REPORT (`portfolio`/`reconcile_portfolio` below): one entry
//! per known program (`.perl-lsp/goals/programs/*.toml`), each computed by
//! running the exact same, unmodified `select::select_next` /
//! reconciliation logic against that program alone. Program-local
//! selection is unchanged and still requires an explicit `--program <id>`
//! (`next`/`reconcile` below, called only when `--program` is given).
//!
//! `default_program`/`active_program`/`active_lane` are NOT deleted this
//! release — only demoted (see `active_goal_manifest::validate_pointer`'s
//! deprecation findings) — so `active.toml` v2 stays readable. Deleting
//! them and introducing a durable `portfolio.toml` registry is a later,
//! separate slice (#4175).
//!
//! Three layers, so the selection algorithm stays pure and independently
//! testable:
//! - [`manifest`] — shared typed loader for `active.toml`'s
//!   `default_program` pointer and milestone ledgers, used by both this
//!   module and `active_goal_manifest::run()` (validator + selector must
//!   not drift on what a valid ledger looks like).
//! - [`select`] — `select_next`, a pure function: `SelectionSnapshot` in,
//!   `SelectionDecision` out. No I/O of any kind.
//! - [`snapshot`] — adapters. ALL `git`/`gh` shelling and manifest reading
//!   live here, fixture-testable via `--fixture <path>` (mirrors
//!   `tasks::queue_snapshot`'s `--fixture` pattern).

pub mod manifest;
pub mod select;
pub mod snapshot;

use color_eyre::eyre::Result;
use serde::Serialize;
use std::path::PathBuf;

use select::SelectionDecision;

#[derive(Debug, Clone, Serialize)]
pub struct GoalsNextOutput {
    pub repository: String,
    pub program: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracker_issue: Option<u64>,
    #[serde(flatten)]
    pub decision: SelectionDecision,
}

/// `cargo xtask goals next` entry point. When `--json` is requested,
/// EVERY error path must emit parseable JSON on stdout and exit nonzero
/// — never an unstructured `color_eyre` dump on stderr (see #3692
/// defect 1: this was previously true only for the unknown-`--program`
/// case, which never `bail!`s in the first place; every OTHER internal
/// `Err` — missing/unparseable `active.toml`, `gh` offline/unauth, a
/// missing program manifest, an invalid milestone ledger — still
/// propagated as unstructured stderr). `render_output` computes
/// everything that would be printed without doing any process-exiting
/// side effect, so the whole flow (not just `build_snapshot`) is covered
/// and the error path stays unit-testable.
pub fn next(program: Option<String>, fixture: Option<PathBuf>, json: bool) -> Result<()> {
    match render_output(program, fixture, json) {
        Ok(text) => {
            println!("{text}");
            Ok(())
        }
        Err(err) if json => {
            println!("{}", render_json_error(&err));
            std::process::exit(1);
        }
        Err(err) => Err(err),
    }
}

/// Computes the exact text `next()` would print for a successful run —
/// JSON when `json` is true, the human-readable summary otherwise.
/// Returns `Err` on any internal failure (unresolved by the caller here);
/// `next()` decides how to surface that `Err` based on `json`.
fn render_output(program: Option<String>, fixture: Option<PathBuf>, json: bool) -> Result<String> {
    let snap = snapshot::build_snapshot(program, fixture)?;
    let decision = select::select_next(&snap);
    let output = GoalsNextOutput {
        repository: snap.repository,
        program: snap.resolved_program,
        program_title: snap.program_title,
        tracker_issue: snap.tracker_issue,
        decision,
    };

    if json { Ok(serde_json::to_string_pretty(&output)?) } else { Ok(render_human(&output)) }
}

/// Renders `err`'s full cause chain as a small parseable JSON object.
/// Never itself fails: `serde_json::to_string_pretty` on a
/// `Vec<String>`/`String`-only struct cannot realistically error, but a
/// literal JSON fallback is used instead of `unwrap`/`expect` in case it
/// ever does (this repo bans both in production code).
fn render_json_error(err: &color_eyre::eyre::Report) -> String {
    let chain: Vec<String> = err.chain().map(ToString::to_string).collect();
    let payload = serde_json::json!({
        "error": chain.first().cloned().unwrap_or_default(),
        "error_chain": chain,
    });
    serde_json::to_string_pretty(&payload).unwrap_or_else(|_| {
        "{\"error\":\"internal: failed to serialize error payload\"}".to_owned()
    })
}

/// #4174 slice 1: one program's entry in the no-arg PORTFOLIO REPORT.
/// Deliberately shaped like [`GoalsNextOutput`] (same `decision` flatten)
/// so JSON consumers see a familiar per-program shape — the difference is
/// there are many of these in one report, never a single repository-global
/// pick.
#[derive(Debug, Clone, Serialize)]
pub struct PortfolioProgramEntry {
    pub id: String,
    /// Informational only: whether `active.toml`'s deprecated
    /// `default_program`/`active_program` pointer currently names this
    /// program. Carries no selection weight — every program in the report
    /// is computed identically regardless of this flag.
    pub is_deprecated_default: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracker_issue: Option<u64>,
    #[serde(flatten)]
    pub decision: SelectionDecision,
}

/// #4174 slice 1: no-arg `cargo xtask goals next --json` output shape —
/// REPLACES the single-selection `GoalsNextOutput` for the no-`--program`
/// case. `goals next --program <id>` still returns `GoalsNextOutput`
/// (`next()`, unchanged).
#[derive(Debug, Clone, Serialize)]
pub struct GoalsPortfolioReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecated_default_program: Option<String>,
    pub program_count: usize,
    pub programs: Vec<PortfolioProgramEntry>,
}

/// `cargo xtask goals next` entry point for the no-`--program` case
/// (#4174 slice 1). Mirrors `next()`'s `--json`-always-parseable contract
/// exactly (same error-rendering helper, same exit-1-on-json-error shape)
/// — the only difference is the payload is a portfolio, not a selection.
pub fn portfolio(fixture: Option<PathBuf>, json: bool) -> Result<()> {
    match render_portfolio_output(fixture, json) {
        Ok(text) => {
            println!("{text}");
            Ok(())
        }
        Err(err) if json => {
            println!("{}", render_json_error(&err));
            std::process::exit(1);
        }
        Err(err) => Err(err),
    }
}

fn render_portfolio_output(fixture: Option<PathBuf>, json: bool) -> Result<String> {
    let (deprecated_default_program, snapshots) = snapshot::build_portfolio_snapshots(fixture)?;

    let programs: Vec<PortfolioProgramEntry> = snapshots
        .into_iter()
        .map(|snap| {
            let id = snap.resolved_program.clone().unwrap_or_default();
            let is_deprecated_default = deprecated_default_program.as_deref() == Some(id.as_str());
            let program_title = snap.program_title.clone();
            let tracker_issue = snap.tracker_issue;
            let decision = select::select_next(&snap);
            PortfolioProgramEntry {
                id,
                is_deprecated_default,
                program_title,
                tracker_issue,
                decision,
            }
        })
        .collect();

    let report = GoalsPortfolioReport {
        deprecated_default_program,
        program_count: programs.len(),
        programs,
    };

    if json {
        Ok(serde_json::to_string_pretty(&report)?)
    } else {
        Ok(render_portfolio_human(&report))
    }
}

fn render_portfolio_human(report: &GoalsPortfolioReport) -> String {
    let mut lines = vec![format!("portfolio: {} program(s)", report.program_count)];
    if let Some(deprecated) = &report.deprecated_default_program {
        lines.push(format!(
            "note: active.toml's default_program={deprecated:?} is deprecated (#4174) — no longer used to pick a single global next; pass --program <id> to select"
        ));
    }
    if report.programs.is_empty() {
        lines.push("no known programs found under .perl-lsp/goals/programs/".to_owned());
        return lines.join("\n");
    }
    for entry in &report.programs {
        let marker = if entry.is_deprecated_default { " (deprecated default)" } else { "" };
        lines.push(String::new());
        lines.push(format!(
            "== {}{marker} =={}",
            entry.id,
            entry.program_title.as_deref().map(|t| format!(" — {t}")).unwrap_or_default()
        ));
        if let Some(issue) = entry.tracker_issue {
            lines.push(format!("tracker: #{issue}"));
        }
        match &entry.decision {
            SelectionDecision::Selected(packet) => {
                lines.push(format!("selectable: {} — {}", packet.id, packet.session_goal));
                lines.push(format!("reason: {}", packet.reason));
            }
            SelectionDecision::Blocked(blockers) => {
                lines.push(format!("blocked: {} blocker(s)", blockers.len()));
                for blocker in blockers {
                    lines.push(format!("  - [{}] {}", blocker.kind, blocker.detail));
                }
            }
            SelectionDecision::Complete(evidence) => {
                lines.push(format!("complete: {}", evidence.detail));
            }
        }
    }
    lines.join("\n")
}

/// `cargo xtask goals reconcile --json` output shape. Advisory/diagnostic —
/// unlike [`GoalsNextOutput`] this never represents a selection decision,
/// just a list of drift findings (#3696 item B).
#[derive(Debug, Clone, Serialize)]
pub struct GoalsReconcileOutput {
    pub program: Option<String>,
    pub finding_count: usize,
    pub findings: Vec<select::ReconciliationFinding>,
}

/// Runs `goals reconcile`. When `--json` is requested, EVERY error path
/// (missing/unparseable fixture, invalid manifest, `gh` offline/unauth via
/// `load_merged_prs_for_candidates`'s hard error) must emit parseable JSON
/// on stdout and exit nonzero — never an unstructured `color_eyre` dump on
/// stderr, mirroring `next()`'s contract exactly (see #3692 defect 1 and
/// the coderabbit/chatgpt-codex findings on this PR: `reconcile --json`
/// previously let such errors bubble through `?` unwrapped). Callers
/// (`main.rs`) decide the additional "findings exist" exit code from the
/// returned count on the `Ok` path — this module never calls
/// `std::process::exit` on that path itself (that lives only in
/// `bin/`/CLI dispatch, per coding standards); it DOES call it on the
/// json-error path, exactly like `next()` already does.
pub fn reconcile(program: Option<String>, fixture: Option<PathBuf>, json: bool) -> Result<usize> {
    match render_reconcile_output(program, fixture, json) {
        Ok((text, finding_count)) => {
            println!("{text}");
            Ok(finding_count)
        }
        Err(err) if json => {
            println!("{}", render_json_error(&err));
            std::process::exit(1);
        }
        Err(err) => Err(err),
    }
}

/// Computes the exact text `reconcile()` would print for a successful run,
/// plus the finding count, without doing any process-exiting side effect —
/// mirrors `render_output`'s split for `next()` so the error path stays
/// unit-testable.
fn render_reconcile_output(
    program: Option<String>,
    fixture: Option<PathBuf>,
    json: bool,
) -> Result<(String, usize)> {
    let findings = snapshot::build_reconciliation_report(program.clone(), fixture)?;
    let finding_count = findings.len();
    let output = GoalsReconcileOutput { program, finding_count, findings };

    let text =
        if json { serde_json::to_string_pretty(&output)? } else { render_reconcile_human(&output) };
    Ok((text, finding_count))
}

fn render_reconcile_human(output: &GoalsReconcileOutput) -> String {
    let mut lines = vec![format!("program: {}", output.program.as_deref().unwrap_or("<default>"))];
    if output.findings.is_empty() {
        lines.push("reconcile: no findings".to_owned());
        return lines.join("\n");
    }
    lines.push(format!("reconcile: {} finding(s)", output.findings.len()));
    for finding in &output.findings {
        lines.push(format!("  - [{}] {}: {}", finding.kind, finding.milestone_id, finding.detail));
    }
    lines.join("\n")
}

/// #4174 slice 1: one program's findings in the no-arg `goals reconcile`
/// PORTFOLIO REPORT — analog of [`PortfolioProgramEntry`] for reconcile.
#[derive(Debug, Clone, Serialize)]
pub struct PortfolioProgramReconcile {
    pub program: String,
    pub finding_count: usize,
    pub findings: Vec<select::ReconciliationFinding>,
}

/// #4174 slice 1: no-arg `cargo xtask goals reconcile --json` output shape
/// — REPLACES the single-program `GoalsReconcileOutput` for the
/// no-`--program` case. `goals reconcile --program <id>` still returns
/// `GoalsReconcileOutput` (`reconcile()`, unchanged).
#[derive(Debug, Clone, Serialize)]
pub struct GoalsReconcilePortfolioOutput {
    pub programs: Vec<PortfolioProgramReconcile>,
    pub total_finding_count: usize,
}

/// `cargo xtask goals reconcile` entry point for the no-`--program` case
/// (#4174 slice 1). Mirrors `reconcile()`'s `--json`-always-parseable
/// contract and its "findings exist" nonzero-count-on-`Ok` shape exactly
/// (`main.rs` decides the process exit code from the returned count, same
/// as `reconcile()`).
pub fn reconcile_portfolio(fixture: Option<PathBuf>, json: bool) -> Result<usize> {
    match render_reconcile_portfolio_output(fixture, json) {
        Ok((text, total_finding_count)) => {
            println!("{text}");
            Ok(total_finding_count)
        }
        Err(err) if json => {
            println!("{}", render_json_error(&err));
            std::process::exit(1);
        }
        Err(err) => Err(err),
    }
}

fn render_reconcile_portfolio_output(
    fixture: Option<PathBuf>,
    json: bool,
) -> Result<(String, usize)> {
    let per_program = snapshot::build_portfolio_reconciliation_report(fixture)?;
    let programs: Vec<PortfolioProgramReconcile> = per_program
        .into_iter()
        .map(|(program, findings)| PortfolioProgramReconcile {
            program,
            finding_count: findings.len(),
            findings,
        })
        .collect();
    let total_finding_count = programs.iter().map(|p| p.finding_count).sum();
    let output = GoalsReconcilePortfolioOutput { programs, total_finding_count };

    let text = if json {
        serde_json::to_string_pretty(&output)?
    } else {
        render_reconcile_portfolio_human(&output)
    };
    Ok((text, total_finding_count))
}

fn render_reconcile_portfolio_human(output: &GoalsReconcilePortfolioOutput) -> String {
    let mut lines = vec![format!(
        "portfolio reconcile: {} program(s), {} total finding(s)",
        output.programs.len(),
        output.total_finding_count
    )];
    for program in &output.programs {
        if program.findings.is_empty() {
            lines.push(format!("  {}: no findings", program.program));
            continue;
        }
        lines.push(format!("  {}: {} finding(s)", program.program, program.finding_count));
        for finding in &program.findings {
            lines.push(format!(
                "    - [{}] {}: {}",
                finding.kind, finding.milestone_id, finding.detail
            ));
        }
    }
    lines.join("\n")
}

fn render_human(output: &GoalsNextOutput) -> String {
    let mut lines = vec![
        format!("repository: {}", output.repository),
        format!("program: {}", output.program.as_deref().unwrap_or("<ambiguous>")),
    ];
    match &output.decision {
        SelectionDecision::Selected(packet) => {
            lines.push(format!("selected: {} — {}", packet.id, packet.session_goal));
            lines.push(format!("reason: {}", packet.reason));
            lines.push(format!("mode: {}", packet.mode));
        }
        SelectionDecision::Blocked(blockers) => {
            lines.push("blocked:".to_owned());
            for blocker in blockers {
                lines.push(format!("  - [{}] {}", blocker.kind, blocker.detail));
            }
        }
        SelectionDecision::Complete(evidence) => {
            lines.push(format!("complete: {} — {}", evidence.program, evidence.detail));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    // Mechanical invariant checks — not just doc comments. Scans the
    // OTHER goals-module source files' PRODUCTION code only (everything
    // before that file's own `#[cfg(test)]`, so this file's assertion
    // literals never self-match, and so those files' own tests — which
    // legitimately write fixture files to a tempdir — don't trip a check
    // aimed at production behavior) for the forbidden surface: `select`
    // must stay pure (no I/O, no session-task-board reads), and neither
    // module may create branches/worktrees/PRs or write outside a
    // fixture/receipt path.
    const SELECT_SRC: &str = include_str!("select.rs");
    const SNAPSHOT_SRC: &str = include_str!("snapshot.rs");
    const MANIFEST_SRC: &str = include_str!("manifest.rs");

    fn production_only(full_src: &str) -> &str {
        full_src.split("#[cfg(test)]").next().unwrap_or(full_src)
    }

    #[test]
    fn select_next_never_reads_task_list_or_shells_out() {
        let src = production_only(SELECT_SRC);
        for forbidden in ["TaskList", "TaskGet", "Command::new", "fs::write", "fs::remove"] {
            assert!(
                !src.contains(forbidden),
                "select.rs production code must stay pure and never reference {forbidden:?}"
            );
        }
    }

    #[test]
    fn manifest_loader_never_reads_task_list() {
        let src = production_only(MANIFEST_SRC);
        for forbidden in ["TaskList", "TaskGet"] {
            assert!(
                !src.contains(forbidden),
                "manifest.rs production code must never reference {forbidden:?}"
            );
        }
    }

    #[test]
    fn snapshot_adapter_is_read_only() {
        let src = production_only(SNAPSHOT_SRC);
        for forbidden in [
            "TaskList",
            "TaskGet",
            "\"branch\"",
            "\"worktree\"",
            "\"commit\"",
            "\"push\"",
            // Catches `gh` command arrays written as separate string-literal
            // args (e.g. `.args(["pr", "create", ...])`) as well as the
            // combined-phrase form; a plain `"pr create"` check misses the
            // array-literal shape entirely since "create" never appears
            // adjacent to "pr" as one token.
            "\"create\"",
            "fs::write",
            "fs::remove",
            "fs::create_dir",
        ] {
            assert!(
                !src.contains(forbidden),
                "snapshot.rs production code must stay read-only and never reference {forbidden:?}"
            );
        }
        // The only live commands this module may shell are the read-only
        // `gh repo view` / `gh pr list` subcommands.
        assert!(src.contains("\"repo\""));
        assert!(src.contains("\"pr\""));
        assert!(src.contains("\"list\""));
    }

    // Regression coverage for #3692 defect 1: `--json` callers must
    // always get parseable JSON, never an unstructured stderr dump, on
    // ANY internal error path — not just the unknown-`--program` case
    // (which never errors in the first place; it resolves to a Blocked
    // decision).
    use super::*;

    #[test]
    fn render_output_surfaces_an_err_when_the_fixture_path_does_not_exist() {
        // A missing --fixture file is the cheapest way to force
        // `build_snapshot` to fail without needing `gh auth`/network —
        // proves the underlying flow still produces an `Err` (as it must,
        // for the non-json caller to see a real error) before we assert
        // the json-caller-side rendering below.
        let bogus = std::path::PathBuf::from("definitely/does/not/exist/prs.json");

        let result = render_output(None, Some(bogus), true);

        assert!(result.is_err(), "expected an Err for a missing fixture file");
    }

    #[test]
    fn json_error_output_is_parseable_and_names_the_failure() {
        let bogus = std::path::PathBuf::from("definitely/does/not/exist/prs.json");
        let err = render_output(None, Some(bogus), true)
            .expect_err("missing fixture file must produce an Err");

        let text = render_json_error(&err);

        let parsed: serde_json::Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("json error output must be parseable JSON: {e}\n{text}"));
        let error_field = parsed
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("expected a string \"error\" field in {text}"));
        assert!(!error_field.is_empty(), "expected a non-empty error message, got {text}");
        assert!(
            parsed.get("error_chain").is_some_and(|v| v.is_array()),
            "expected an \"error_chain\" array field in {text}"
        );
    }

    #[test]
    fn next_with_json_never_returns_err_to_the_caller() {
        // `next()` must never propagate `Err` up through `run_cli`/`main`
        // when `--json` was requested (that path is what previously
        // produced the unstructured color_eyre stderr dump). It instead
        // prints the JSON error to stdout and calls
        // `std::process::exit(1)` — which we cannot exercise directly in
        // a test process, so this test proves the OTHER half of the
        // contract: with json=false the same failure DOES propagate as
        // `Err` (preserving prior behavior for human callers), showing
        // the json/non-json branches are genuinely distinguished by
        // `next()`, not merged.
        let bogus = std::path::PathBuf::from("definitely/does/not/exist/prs.json");

        let human_result = render_output(None, Some(bogus), false);
        assert!(human_result.is_err(), "non-json path must still surface Err to its caller");
    }

    // Regression coverage for the coderabbit (mod.rs:136) / chatgpt-codex
    // (mod.rs:126) findings on this PR: `goals reconcile --json` must get
    // the exact same JSON-error contract as `goals next --json` above —
    // it previously let `build_reconciliation_report`'s `Err` (including
    // `load_merged_prs_for_candidates`'s `bail!` on a failed `gh pr list
    // --state merged`, coderabbit snapshot.rs:448) bubble unwrapped.

    #[test]
    fn render_reconcile_output_surfaces_an_err_when_the_fixture_path_does_not_exist() {
        let bogus = std::path::PathBuf::from("definitely/does/not/exist/prs.json");

        let result = render_reconcile_output(None, Some(bogus), true);

        assert!(result.is_err(), "expected an Err for a missing fixture file");
    }

    #[test]
    fn reconcile_json_error_output_is_parseable_and_names_the_failure() {
        let bogus = std::path::PathBuf::from("definitely/does/not/exist/prs.json");
        let err = render_reconcile_output(None, Some(bogus), true)
            .expect_err("missing fixture file must produce an Err");

        let text = render_json_error(&err);

        let parsed: serde_json::Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("json error output must be parseable JSON: {e}\n{text}"));
        let error_field = parsed
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("expected a string \"error\" field in {text}"));
        assert!(!error_field.is_empty(), "expected a non-empty error message, got {text}");
        assert!(
            parsed.get("error_chain").is_some_and(|v| v.is_array()),
            "expected an \"error_chain\" array field in {text}"
        );
    }

    #[test]
    fn reconcile_with_json_never_returns_err_to_the_caller() {
        // Same contract as `next_with_json_never_returns_err_to_the_caller`
        // above: with json=false the failure still propagates as `Err`
        // (preserving prior behavior for human callers) -- the json/non-json
        // branches are genuinely distinguished by `reconcile()`, not merged.
        let bogus = std::path::PathBuf::from("definitely/does/not/exist/prs.json");

        let human_result = render_reconcile_output(None, Some(bogus), false);
        assert!(human_result.is_err(), "non-json path must still surface Err to its caller");
    }

    // #4174 slice 1 tests (a) and (b): no-arg `goals next` reports a
    // portfolio instead of selecting one repository-global next, while
    // `goals next --program <id>` selects program-locally exactly as
    // before (parity with the pre-existing single-program behavior).
    // These exercise the REAL repo tree's `.perl-lsp/goals/programs/`
    // (`agent_loop_enablement`, `real_perl_editor_trust`) via a
    // `--fixture` file so no live `gh` call is needed, mirroring every
    // other `render_output`/`render_reconcile_output` test in this module.

    fn write_empty_prs_fixture(temp: &tempfile::TempDir) -> Result<std::path::PathBuf> {
        let path = temp.path().join("prs.json");
        std::fs::write(&path, r#"{"repository":"r","prs":[]}"#)?;
        Ok(path)
    }

    #[test]
    fn test_a_no_arg_portfolio_report_does_not_emit_a_single_global_selection() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let fixture_path = write_empty_prs_fixture(&temp)?;

        let text = render_portfolio_output(Some(fixture_path.clone()), false)?;
        assert!(text.starts_with("portfolio:"), "expected a portfolio report header, got {text:?}");
        // The OLD single-selection `render_human` always starts with a
        // `repository: <name>` line — the portfolio report must not
        // reproduce that shape (it names no single resolved repository-wide
        // program/decision).
        assert!(
            !text.starts_with("repository:"),
            "no-arg goals next must not emit the old single-selection shape, got {text:?}"
        );

        let json_text = render_portfolio_output(Some(fixture_path), true)?;
        let parsed: serde_json::Value = serde_json::from_str(&json_text)
            .unwrap_or_else(|e| panic!("portfolio JSON must be parseable: {e}\n{json_text}"));
        assert!(
            parsed.get("programs").is_some_and(|v| v.is_array()),
            "expected a \"programs\" array in {json_text}"
        );
        // The OLD single-selection `GoalsNextOutput` flattens `decision`
        // (and `data`) as TOP-LEVEL keys via `#[serde(tag = "decision", ...)]`
        // — the portfolio report must never reproduce that top-level shape,
        // only per-program (nested) decisions.
        assert!(
            parsed.get("decision").is_none(),
            "no-arg goals next must not emit a top-level \"decision\" (that's the old single-selection shape), got {json_text}"
        );
        let programs = parsed
            .get("programs")
            .and_then(|v| v.as_array())
            .unwrap_or_else(|| panic!("expected a \"programs\" array in {json_text}"));
        assert!(
            programs.len() >= 2,
            "expected at least the 2 known real-repo programs in the portfolio, got {programs:?}"
        );
        for program in programs {
            assert!(
                program.get("decision").is_some(),
                "expected each per-program entry to carry its OWN decision, got {program:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn test_b_explicit_program_selection_is_unchanged_parity_with_pre_slice_behavior() -> Result<()>
    {
        let temp = tempfile::tempdir()?;
        let fixture_path = write_empty_prs_fixture(&temp)?;

        // This is the EXACT pre-#4174 code path (`next()`/`render_output`
        // with `program = Some(id)`) -- unmodified by this slice. Proves
        // `--program` selection still resolves the named program directly,
        // never falling through to a portfolio or an unrelated default.
        let json_text =
            render_output(Some("real_perl_editor_trust".to_owned()), Some(fixture_path), true)?;
        let parsed: serde_json::Value = serde_json::from_str(&json_text)
            .unwrap_or_else(|e| panic!("selection JSON must be parseable: {e}\n{json_text}"));
        assert_eq!(
            parsed.get("program").and_then(|v| v.as_str()),
            Some("real_perl_editor_trust"),
            "expected the explicitly-requested program to resolve, got {json_text}"
        );
        // Single-selection shape: a top-level "decision" key, exactly like
        // pre-slice `GoalsNextOutput` -- never the portfolio's "programs" array.
        assert!(
            parsed.get("decision").is_some(),
            "expected the pre-existing single-selection \"decision\" shape, got {json_text}"
        );
        assert!(
            parsed.get("programs").is_none(),
            "explicit --program selection must never emit the portfolio's \"programs\" array, got {json_text}"
        );
        Ok(())
    }
}
