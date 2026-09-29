//! `cargo xtask pr-ledger generate` — PR reconciliation ledger generator.
//!
//! Shells to `gh api` for one or more repositories and emits:
//!
//! 1. Per-repo `<repo-slug>.json` skeleton ledger arrays under `--out` directory.
//! 2. Per-repo `<repo-slug>.receipt.json` canonical pagination receipts.
//! 3. A combined `pr-ledger.md` markdown summary table.
//!
//! Skeleton rows have `classification: "unclassified"` and empty `evidence: []`
//! so scouts can fill in classifications without starting from scratch.
//!
//! The output is a **worklist** — not a ground-truth ledger.  Scouts read it,
//! fill `classification` and `evidence`, then the validator (`cargo xtask agent
//! ledgers validate`) enforces correctness.
//!
//! # Pagination completeness (#15345)
//!
//! Every fetch is driven through `gh api --paginate --slurp` and produces a
//! [`PaginationReceipt`] alongside the rows. The receipt carries the source
//! endpoint, the page parameters, the observed completeness, the dedup
//! outcome, and a digest over the ordered PR identities. Consumers may use
//! `completeness == "complete"` to make progress claims; bounded selections
//! must be requested explicitly and cannot be relabelled as the repository
//! backlog.
//!
//! # Schema
//!
//! Each row conforms to the ORCHESTRATION_ROLES.md builder/closer output schema.
//! This row type and the `pr-triage.v1` contract in `agent_ledgers` are the single
//! authority for PR-triage rows (#15557); the competing contract formerly at
//! `docs/agents/pr-ledger.schema.json` was retired, its structured `close_proof`
//! half now owned by the validator.
//!
//! ```json
//! {
//!   "pr": "1234",
//!   "title": "fix: something (#1234)",
//!   "surface_guess": "xtask",
//!   "classification": "unclassified",
//!   "confidence": "low",
//!   "evidence": [],
//!   "cleanup_done": false,
//!   "known_gaps": [],
//!   "is_draft": false,
//!   "mergeable": "unknown",
//!   "head_ref": "feat/1234-thing",
//!   "author": "EffortlessSteven"
//! }
//! ```
//!
//! `mergeable` is `"unknown"` for list-sourced rows: the pulls list endpoint
//! exposes no mergeability field (see `RestPull`).
//!
//! # Exit codes
//! - `0` — generation succeeded (completeness recorded in the receipt).
//! - `1` — error (gh not available, bad repo, write failure).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::Utc;
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// Schema constants
// ---------------------------------------------------------------------------

/// Version of the pagination-receipt envelope. Bumped whenever the receipt
/// shape changes incompatibly; consumers may pin against this string.
pub const SCHEMA_VERSION: &str = "pr-ledger-pagination-receipt.v1";

/// REST API page size requested from `gh api`. 100 is the GitHub max for the
/// pulls endpoint, so this is also the natural upper bound per page.
pub const PAGE_SIZE: usize = 100;

/// Hard cap on pages requested per repo. A repository with more than this many
/// open PRs (currently 50,000) returns a `truncated` receipt rather than
/// silently looping forever. This is a defensive ceiling, not a routine
/// expected scale — most repositories finish in a handful of pages.
pub const MAX_PAGES_PER_REPO: usize = 500;

// ---------------------------------------------------------------------------
// Public config
// ---------------------------------------------------------------------------

/// Configuration for `pr-ledger generate`.
pub struct GenerateConfig {
    /// Repositories to query (owner/name format).
    pub repos: Vec<String>,
    /// Output directory for generated artifacts. Defaults to
    /// `target/reconciliation/`.
    pub out: PathBuf,
    /// Optional fixture JSON path (one per repo, for testing without live gh).
    /// When set, the fixture is used instead of shelling to gh. The fixture
    /// shape is an array of `RestPull` JSON objects (single-page semantics for
    /// tests; see also `paginated_fixture`).
    pub fixture: Option<PathBuf>,
    /// Optional paginated fixture path used in tests to drive the multi-page
    /// pagination path without shelling to gh. The fixture shape is an array
    /// of pages, where each page is an array of `RestPull` JSON objects.
    pub paginated_fixture: Option<PathBuf>,
    /// When true, the fetch call is allowed to use a fake clock anchor for
    /// `observed_at`. Tests use this to make receipts byte-deterministic.
    pub deterministic_clock: bool,
}

// ---------------------------------------------------------------------------
// Row type
// ---------------------------------------------------------------------------

/// A single PR row in the reconciliation ledger.
///
/// Fields align with the ORCHESTRATION_ROLES.md output schema for builder/closer agents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LedgerRow {
    /// PR number as string (the `pr-triage.v1` contract type).
    pub pr: String,
    /// PR title.
    pub title: String,
    /// Best-guess surface area inferred from labels + title prefix.
    pub surface_guess: String,
    /// Classification — starts as "unclassified"; scouts fill in.
    pub classification: String,
    /// Confidence — starts as "low".
    pub confidence: String,
    /// Evidence citations — starts empty.
    pub evidence: Vec<String>,
    /// Cleanup done — starts false.
    pub cleanup_done: bool,
    /// Known gaps — starts empty.
    pub known_gaps: Vec<String>,
    /// Whether the PR is a draft.
    pub is_draft: bool,
    /// Mergeability status from GitHub.
    pub mergeable: String,
    /// Head branch ref name.
    pub head_ref: String,
    /// PR author login.
    pub author: String,
}

// ---------------------------------------------------------------------------
// Pagination receipt
// ---------------------------------------------------------------------------

/// A canonical, pagination-complete backlog receipt (#15345).
///
/// The receipt is the authoritative evidence for "is this a complete
/// inventory of the repository backlog at observation time?" Rows derived
/// from a fetch are *only* as authoritative as the receipt they came from.
///
/// `schema_version` lets consumers refuse unknown envelopes; the rest of the
/// fields are positional evidence the issue body requires.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaginationReceipt {
    /// Schema version of this receipt envelope.
    pub schema_version: String,
    /// Repository slug in `owner/name` form.
    pub repository: String,
    /// State filter used (e.g. "open").
    pub query_state: String,
    /// When the inventory was observed, ISO 8601 UTC with second precision.
    pub observed_at: String,
    /// Source endpoint used (CLI shape, e.g. `gh api repos/{owner}/{repo}/pulls`).
    pub source_endpoint: String,
    /// API generation identifier (`v3` for REST).
    pub api_generation: String,
    /// Reported total count when the API exposes one; `None` for endpoints
    /// (such as the pulls list) that do not return a count header.
    pub reported_total: Option<u64>,
    /// Unique PR identities actually fetched after dedup.
    pub fetched_unique: usize,
    /// Pages requested and accepted.
    pub pages_requested: usize,
    /// Page size used per request.
    pub page_size: usize,
    /// Link/cursor identity marker. REST page numbers are not stable across
    /// retries, so we record a stable opaque token (`rest-page-N`) per page.
    pub page_identity: Vec<String>,
    /// Completeness outcome.
    pub completeness: Completeness,
    /// Whether the same PR identity appeared on more than one page (a sign
    /// the backlog mutated during traversal).
    pub snapshot_drift: bool,
    /// SHA-256 digest over the ordered PR identities, lowercase hex.
    pub cursor_digest: String,
    /// Ordered PR numbers as they appeared in the inventory.
    pub ordered_identities: Vec<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Completeness {
    /// All pages were fetched successfully and no `next` link remained.
    Complete,
    /// One or more pages failed or `MAX_PAGES_PER_REPO` was hit.
    Truncated,
    /// The API could not be reached (`gh` missing, transport failure).
    SourceUnavailable,
    /// The set changed during traversal (duplicate identities detected).
    SnapshotDrift,
}

impl Completeness {
    /// Returns true when the receipt is safe to use as a complete denominator.
    pub fn is_complete(&self) -> bool {
        matches!(self, Completeness::Complete)
    }

    /// Returns true when the receipt is *not* safe to use as a complete
    /// denominator and consumers must treat the rows as a bounded selection.
    pub fn is_partial(&self) -> bool {
        !self.is_complete()
    }
}

// ---------------------------------------------------------------------------
// Raw GitHub PR shape (what gh returns)
// ---------------------------------------------------------------------------

/// One entry of the REST `GET repos/{owner}/{repo}/pulls` list response.
/// Field names follow the REST representation (`draft`, `user.login`,
/// `head.ref`), NOT the `gh pr list --json` GraphQL shape (`isDraft`,
/// `author`, `headRefName`): parsing list output as the latter fails on
/// every live page (#15345 review).
///
/// The list endpoint exposes no mergeability field (that lives on the
/// single-PR representation), so rows honestly record `"unknown"`; detail
/// enrichment can fill it in later.
#[derive(Debug, Clone, Deserialize, PartialEq)]
struct RestPull {
    pub number: u64,
    pub title: String,
    pub labels: Vec<GhLabel>,
    pub draft: bool,
    pub head: RestHead,
    pub user: GhAuthor,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
struct RestHead {
    #[serde(rename = "ref")]
    pub ref_name: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
struct GhLabel {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
struct GhAuthor {
    pub login: String,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn generate(config: GenerateConfig) -> Result<()> {
    fs::create_dir_all(&config.out)
        .with_context(|| format!("creating output directory {}", config.out.display()))?;

    if config.deterministic_clock && config.fixture.is_none() && config.paginated_fixture.is_none()
    {
        bail!(
            "--deterministic-clock is test-only: it pins receipts to the 1970 anchor, \
             which would masquerade a live observation time. Use it with --fixture or \
             --paginated-fixture."
        );
    }

    let mut all_rows: Vec<(String, Vec<LedgerRow>, Completeness)> = Vec::new();

    for repo in &config.repos {
        let outcome = if let Some(ref fixture) = config.fixture {
            load_outcome_from_fixture(fixture, repo, config.deterministic_clock)?
        } else if let Some(ref paginated) = config.paginated_fixture {
            load_outcome_from_paginated_fixture(paginated, repo, config.deterministic_clock)?
        } else {
            fetch_outcome_from_gh(repo)?
        };

        let rows: Vec<LedgerRow> = outcome.prs.into_iter().map(|pr| shape_row(pr, repo)).collect();

        write_repo_json(&rows, repo, &config.out)?;
        write_repo_receipt(&outcome.receipt, repo, &config.out)?;
        all_rows.push((repo.clone(), rows, outcome.receipt.completeness));

        if outcome.receipt.completeness.is_partial() {
            eprintln!(
                "warning: pr-ledger inventory for {repo} is {} (fetched_unique={}, pages={}); \
                 downstream claims of 'total' or 'all' must be qualified",
                receipt_completeness_label(&outcome.receipt.completeness),
                outcome.receipt.fetched_unique,
                outcome.receipt.pages_requested,
            );
        }
    }

    write_summary_md(&all_rows, &config.out)?;

    println!(
        "pr-ledger generate: wrote {} repo(s) to {}",
        config.repos.len(),
        config.out.display()
    );
    Ok(())
}

fn receipt_completeness_label(c: &Completeness) -> &'static str {
    match c {
        Completeness::Complete => "complete",
        Completeness::Truncated => "truncated",
        Completeness::SourceUnavailable => "source_unavailable",
        Completeness::SnapshotDrift => "snapshot_drift",
    }
}

// ---------------------------------------------------------------------------
// gh invocation (paginated)
// ---------------------------------------------------------------------------

/// Fetch one REST list page: `GET repos/{owner}/{repo}/pulls` with explicit
/// query parameters. `--method GET` is load-bearing: `gh api` defaults to
/// POST once `-F` parameters are present, and POST on the pulls collection
/// is the pull-*creation* endpoint, so every live fetch failed before any
/// ledger was generated (#15345 review).
fn fetch_page(repo: &str, page_number: u64) -> Result<serde_json::Value> {
    let endpoint = format!("repos/{repo}/pulls");

    let output = Command::new("gh")
        .args([
            "api",
            endpoint.as_str(),
            "--method",
            "GET",
            "-H",
            "Accept: application/vnd.github+json",
            "-F",
            "state=open",
            "-F",
            &format!("per_page={PAGE_SIZE}"),
            "-F",
            &format!("page={page_number}"),
        ])
        .output()
        .with_context(|| format!("running `gh api {endpoint}` page {page_number} for {repo}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("gh api page {page_number} failed for {repo}: {stderr}");
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&raw)
        .with_context(|| format!("parsing gh api page {page_number} for {repo}"))
}

/// Drive pagination page-by-page with an explicit counter instead of
/// `gh api --paginate`, which buffers every page before any cap can apply.
/// Stops at the first short page (fewer than `PAGE_SIZE` items, including
/// the empty terminal probe that settles exact page multiples as complete)
/// or at `MAX_PAGES_PER_REPO`. A full trailing page therefore always means
/// the cap stopped traversal, which is exactly what `assemble_outcome`'s
/// `last_page_full` rule reports as `Truncated`. The fetcher is injectable
/// so the stop discipline is unit-provable without shelling out.
fn collect_pages(
    mut fetch_one: impl FnMut(u64) -> Result<serde_json::Value>,
) -> Result<Vec<serde_json::Value>> {
    let mut pages = Vec::new();
    let mut page_number = 1u64;
    loop {
        let page_value = fetch_one(page_number)?;
        let page_len = page_value.as_array().map(|a| a.len()).unwrap_or(0);
        pages.push(page_value);
        if page_len < PAGE_SIZE || pages.len() >= MAX_PAGES_PER_REPO {
            return Ok(pages);
        }
        page_number += 1;
    }
}

fn fetch_outcome_from_gh(repo: &str) -> Result<FetchOutcome> {
    // Live receipts always carry the real observation time: the
    // deterministic anchor belongs to fixture-backed runs only.
    let pages = collect_pages(|page_number| fetch_page(repo, page_number))?;
    assemble_outcome(serde_json::Value::from(pages), repo, false, "gh api")
}

fn load_outcome_from_fixture(
    path: &Path,
    repo: &str,
    deterministic_clock: bool,
) -> Result<FetchOutcome> {
    let content =
        fs::read_to_string(path).with_context(|| format!("reading fixture {}", path.display()))?;
    let page_value: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("parsing fixture JSON {}", path.display()))?;
    let pages = serde_json::json!([page_value]);
    assemble_outcome(pages, repo, deterministic_clock, "fixture")
}

fn load_outcome_from_paginated_fixture(
    path: &Path,
    repo: &str,
    deterministic_clock: bool,
) -> Result<FetchOutcome> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("reading paginated fixture {}", path.display()))?;
    let pages: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("parsing paginated fixture JSON {}", path.display()))?;
    assemble_outcome(pages, repo, deterministic_clock, "fixture")
}

/// Reduce a raw multi-page JSON document into a [`FetchOutcome`].
fn assemble_outcome(
    pages_value: serde_json::Value,
    repo: &str,
    deterministic_clock: bool,
    source_label: &str,
) -> Result<FetchOutcome> {
    let pages_array = pages_value
        .as_array()
        .ok_or_else(|| color_eyre::eyre::eyre!("paginated output is not a JSON array"))?;

    if pages_array.len() > MAX_PAGES_PER_REPO {
        bail!(
            "paginated output for {repo} has {} pages (max {})",
            pages_array.len(),
            MAX_PAGES_PER_REPO
        );
    }

    let mut all_prs: Vec<RestPull> = Vec::new();
    let mut seen_numbers: BTreeSet<u64> = BTreeSet::new();
    let mut page_identity: Vec<String> = Vec::with_capacity(pages_array.len());
    let mut snapshot_drift = false;

    for (page_idx, page_value) in pages_array.iter().enumerate() {
        let page_array = page_value.as_array().ok_or_else(|| {
            color_eyre::eyre::eyre!("page {page_idx} of {repo} response is not an array")
        })?;

        let items: Vec<RestPull> =
            serde_json::from_value(serde_json::Value::from(page_array.clone()))
                .with_context(|| format!("parsing page {page_idx} of {repo}"))?;

        for pr in items {
            if !seen_numbers.insert(pr.number) {
                // The REST API should not return the same PR on two pages,
                // but if it does, that's drift evidence.
                snapshot_drift = true;
                continue;
            }
            all_prs.push(pr);
        }

        page_identity.push(format!("rest-page-{page_idx}"));
    }

    let observed_at = if deterministic_clock {
        // Tests can pin the clock to make receipts byte-deterministic across
        // runs that share the same canonical input.
        "1970-01-01T00:00:00Z".to_string()
    } else {
        Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    };

    let ordered_identities: Vec<u64> = all_prs.iter().map(|p| p.number).collect();
    let cursor_digest = compute_digest(&ordered_identities);

    let fetched_unique = ordered_identities.len();
    let pages_requested = pages_array.len();
    let last_page_full = pages_array
        .last()
        .and_then(|p| p.as_array().map(|a| a.len() == PAGE_SIZE))
        .unwrap_or(false);

    let completeness = if snapshot_drift {
        Completeness::SnapshotDrift
    } else if pages_requested == 0 {
        // Zero pages is a valid outcome only when the API was reachable; a
        // missing payload from `gh api` would have already failed at the
        // `serde_json::from_str` step above. Record it as `complete` with
        // zero rows.
        Completeness::Complete
    } else if last_page_full {
        // Defensive: if the last page returned exactly PAGE_SIZE items, the
        // server may still have more pages but `gh api --paginate` did not
        // see a `next` link. We refuse to claim completeness in that case.
        Completeness::Truncated
    } else {
        Completeness::Complete
    };

    let receipt = PaginationReceipt {
        schema_version: SCHEMA_VERSION.to_string(),
        repository: repo.to_string(),
        query_state: "open".to_string(),
        observed_at,
        source_endpoint: format!("{source_label} repos/{repo}/pulls"),
        api_generation: "v3".to_string(),
        reported_total: None,
        fetched_unique,
        pages_requested,
        page_size: PAGE_SIZE,
        page_identity,
        completeness,
        snapshot_drift,
        cursor_digest,
        ordered_identities,
    };

    Ok(FetchOutcome { prs: all_prs, receipt })
}

struct FetchOutcome {
    prs: Vec<RestPull>,
    receipt: PaginationReceipt,
}

fn compute_digest(ordered: &[u64]) -> String {
    let mut hasher = Sha256::new();
    for n in ordered {
        hasher.update(n.to_be_bytes());
        hasher.update(b"\n");
    }
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

// ---------------------------------------------------------------------------
// Row shaping
// ---------------------------------------------------------------------------

fn shape_row(pr: RestPull, _repo: &str) -> LedgerRow {
    let label_names: Vec<String> = pr.labels.iter().map(|l| l.name.clone()).collect();
    let surface_guess = infer_surface(&pr.title, &label_names);

    LedgerRow {
        pr: pr.number.to_string(),
        title: pr.title,
        surface_guess,
        classification: "unclassified".to_string(),
        confidence: "low".to_string(),
        evidence: Vec::new(),
        cleanup_done: false,
        known_gaps: Vec::new(),
        is_draft: pr.draft,
        // The pulls list endpoint exposes no mergeability; "unknown" is the
        // honest value until a detail view enriches the row.
        mergeable: "unknown".to_string(),
        head_ref: pr.head.ref_name,
        author: pr.user.login,
    }
}

/// Infer the surface area from conventional commit type prefix in the title
/// or from labels when present.
fn infer_surface(title: &str, labels: &[String]) -> String {
    // Label-based hints take priority.
    for label in labels {
        let l = label.as_str();
        if l.contains("parser") {
            return "parser".to_string();
        }
        if l.contains("lsp") {
            return "lsp".to_string();
        }
        if l.contains("dap") {
            return "dap".to_string();
        }
        if l.contains("xtask") || l.contains("ci") {
            return "xtask".to_string();
        }
        if l.contains("docs") {
            return "docs".to_string();
        }
    }

    // Fall back to conventional-commit scope in the title.
    // Matches patterns like: `fix(parser): ...`, `feat(lsp): ...`, `xtask(agents): ...`
    if let Some(scope_start) = title.find('(')
        && let Some(scope_end) = title[scope_start..].find(')')
    {
        let scope = &title[scope_start + 1..scope_start + scope_end];
        if !scope.is_empty() {
            return scope.to_string();
        }
    }

    "unknown".to_string()
}

// ---------------------------------------------------------------------------
// Output writers
// ---------------------------------------------------------------------------

fn write_repo_json(rows: &[LedgerRow], repo: &str, out_dir: &Path) -> Result<()> {
    // The worklist is written under `target/` and never committed, so the batch
    // ledger validator never globs it (#15557). Enforce the same `pr-triage.v1`
    // contract at the only seam that always executes: generation.
    for row in rows {
        let line = serde_json::to_string(row).context("serializing ledger row")?;
        let errors = crate::tasks::agent_ledgers::validate_pr_triage_row(&line);
        if !errors.is_empty() {
            return Err(color_eyre::eyre::eyre!(
                "row for PR {} violates the `pr-triage.v1` contract: {}",
                row.pr,
                errors.join("; ")
            ));
        }
    }
    let slug = repo.replace('/', "-");
    let path = out_dir.join(format!("{slug}.json"));
    let json = serde_json::to_string_pretty(rows).context("serializing repo ledger JSON")?;
    fs::write(&path, format!("{json}\n"))
        .with_context(|| format!("writing repo ledger to {}", path.display()))?;
    println!("  wrote {}", path.display());
    Ok(())
}

fn write_repo_receipt(receipt: &PaginationReceipt, repo: &str, out_dir: &Path) -> Result<()> {
    let slug = repo.replace('/', "-");
    let path = out_dir.join(format!("{slug}.receipt.json"));
    let json = serde_json::to_string_pretty(receipt)
        .context("serializing repo pagination receipt JSON")?;
    fs::write(&path, format!("{json}\n"))
        .with_context(|| format!("writing repo receipt to {}", path.display()))?;
    println!("  wrote {}", path.display());
    Ok(())
}

fn write_summary_md(
    all_rows: &[(String, Vec<LedgerRow>, Completeness)],
    out_dir: &Path,
) -> Result<()> {
    let path = out_dir.join("pr-ledger.md");
    let mut buf = String::new();

    buf.push_str("# PR Reconciliation Ledger\n\n");
    buf.push_str(
        "> Generated by `cargo xtask pr-ledger generate`. \
         Classification and evidence columns are blank — fill in via scout.\n\n",
    );

    for (repo, rows, completeness) in all_rows {
        buf.push_str(&format!("## {repo}\n\n"));
        // The summary must not let a partial inventory read as a complete
        // backlog: every section carries its receipt completeness state.
        buf.push_str(&format!(
            "*Inventory: {} ({} PRs; see {}.receipt.json).*\n\n",
            receipt_completeness_label(completeness),
            rows.len(),
            repo.replace('/', "-"),
        ));
        if rows.is_empty() {
            buf.push_str("_No open PRs._\n\n");
            continue;
        }
        buf.push_str("| PR | Title | Surface | Draft | Mergeable | Author |\n");
        buf.push_str("|----|-------|---------|-------|-----------|--------|\n");
        for row in rows {
            let draft_mark = if row.is_draft { "yes" } else { "no" };
            buf.push_str(&format!(
                "| #{pr} | {title} | {surface} | {draft} | {mergeable} | {author} |\n",
                pr = row.pr,
                title = md_escape(&row.title),
                surface = row.surface_guess,
                draft = draft_mark,
                mergeable = row.mergeable,
                author = row.author,
            ));
        }
        buf.push('\n');
    }

    fs::write(&path, &buf)
        .with_context(|| format!("writing summary markdown to {}", path.display()))?;
    println!("  wrote {}", path.display());
    Ok(())
}

/// Escape `|` characters in markdown table cells.
fn md_escape(s: &str) -> String {
    s.replace('|', "&#124;")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ----- surface inference ------------------------------------------------

    #[test]
    fn test_infer_surface_from_conventional_scope() -> Result<()> {
        assert_eq!(infer_surface("fix(parser): handle heredoc", &[]), "parser");
        assert_eq!(infer_surface("feat(lsp): add hover", &[]), "lsp");
        assert_eq!(infer_surface("xtask(agents): validate ledgers", &[]), "agents");
        Ok(())
    }

    #[test]
    fn test_infer_surface_from_labels() -> Result<()> {
        let labels = vec!["area/parser".to_string()];
        assert_eq!(infer_surface("unformatted title", &labels), "parser");
        Ok(())
    }

    #[test]
    fn test_infer_surface_labels_beat_scope() -> Result<()> {
        // Label wins over scope.
        let labels = vec!["area/lsp".to_string()];
        assert_eq!(infer_surface("fix(parser): thing", &labels), "lsp");
        Ok(())
    }

    #[test]
    fn test_infer_surface_unknown_fallback() -> Result<()> {
        assert_eq!(infer_surface("no scope or label here", &[]), "unknown");
        Ok(())
    }

    // ----- row shaping from canned gh JSON ----------------------------------

    fn make_rest_pull(number: u64, title: &str, labels: &[&str]) -> RestPull {
        RestPull {
            number,
            title: title.to_string(),
            labels: labels.iter().map(|l| GhLabel { name: l.to_string() }).collect(),
            draft: false,
            head: RestHead { ref_name: format!("feat/{number}-thing") },
            user: GhAuthor { login: "test-user".to_string() },
        }
    }

    #[test]
    fn test_shape_row_defaults() -> Result<()> {
        let pr = make_rest_pull(42, "fix(lsp): hover docs (#42)", &[]);
        let row = shape_row(pr, "EffortlessMetrics/perl-lsp-swarm");

        assert_eq!(row.pr, "42");
        assert_eq!(row.classification, "unclassified");
        assert_eq!(row.confidence, "low");
        assert!(row.evidence.is_empty());
        assert!(!row.cleanup_done);
        assert!(row.known_gaps.is_empty());
        assert_eq!(row.surface_guess, "lsp");
        Ok(())
    }

    #[test]
    fn test_shape_row_draft_preserved() -> Result<()> {
        let mut pr = make_rest_pull(7, "wip: draft (#7)", &[]);
        pr.draft = true;
        let row = shape_row(pr, "owner/repo");
        assert!(row.is_draft);
        Ok(())
    }

    #[test]
    fn test_shape_row_author_preserved() -> Result<()> {
        let pr = make_rest_pull(9, "feat(dap): thing (#9)", &[]);
        let row = shape_row(pr, "owner/repo");
        assert_eq!(row.author, "test-user");
        Ok(())
    }

    #[test]
    fn test_shape_row_mergeable_is_unknown_from_list() -> Result<()> {
        // The pulls list endpoint exposes no mergeability field, so rows
        // honestly record "unknown" instead of fabricating a verdict.
        let pr = make_rest_pull(10, "fix(parser): thing (#10)", &[]);
        let row = shape_row(pr, "owner/repo");
        assert_eq!(row.mergeable, "unknown");
        assert_eq!(row.head_ref, "feat/10-thing");
        Ok(())
    }

    // ----- receipt + pagination assembly ------------------------------------

    // Fixtures use the REST list representation (what `gh api` returns),
    // never the `gh pr list --json` GraphQL shape: only the REST shape
    // exercises the production deserialization boundary (#15345 review).
    fn pr_json(number: u64, title: &str) -> serde_json::Value {
        serde_json::json!({
            "number": number,
            "title": title,
            "labels": [],
            "draft": false,
            "head": {"ref": format!("feat/{number}-thing")},
            "user": {"login": "test-user"}
        })
    }

    fn single_page(prs: Vec<serde_json::Value>) -> serde_json::Value {
        serde_json::json!([prs])
    }

    fn multi_page(pages: Vec<Vec<serde_json::Value>>) -> serde_json::Value {
        serde_json::json!(pages)
    }

    #[test]
    fn test_assemble_outcome_zero_prs_is_complete() -> Result<()> {
        let pages = single_page(vec![]);
        let outcome = assemble_outcome(pages, "owner/repo", true, "fixture")?;

        assert_eq!(outcome.receipt.fetched_unique, 0);
        assert_eq!(outcome.receipt.pages_requested, 1);
        assert_eq!(outcome.receipt.completeness, Completeness::Complete);
        assert!(!outcome.receipt.snapshot_drift);
        assert!(outcome.prs.is_empty());
        Ok(())
    }

    #[test]
    fn test_assemble_outcome_one_pr() -> Result<()> {
        let pages = single_page(vec![pr_json(1, "feat(lsp): one (#1)")]);
        let outcome = assemble_outcome(pages, "owner/repo", true, "fixture")?;

        assert_eq!(outcome.receipt.fetched_unique, 1);
        assert_eq!(outcome.receipt.pages_requested, 1);
        assert_eq!(outcome.receipt.completeness, Completeness::Complete);
        assert_eq!(outcome.receipt.ordered_identities, vec![1]);
        Ok(())
    }

    #[test]
    fn test_assemble_outcome_30_prs_single_page() -> Result<()> {
        let prs: Vec<serde_json::Value> =
            (1..=30).map(|n| pr_json(n, &format!("fix(lsp): n#{n}"))).collect();
        let pages = single_page(prs);
        let outcome = assemble_outcome(pages, "owner/repo", true, "fixture")?;

        assert_eq!(outcome.receipt.fetched_unique, 30);
        assert_eq!(outcome.receipt.pages_requested, 1);
        // 30 < 100, so the single page is not full -> complete.
        assert_eq!(outcome.receipt.completeness, Completeness::Complete);
        assert_eq!(outcome.receipt.ordered_identities.len(), 30);
        Ok(())
    }

    #[test]
    fn test_assemble_outcome_exactly_one_full_page_is_truncated() -> Result<()> {
        // `collect_pages` only stops on a full trailing page at the cap, so
        // a full last page reaching assembly means traversal may have more
        // pages: the receipt refuses completeness. (The live loop would have
        // probed one page further; fixtures drive assembly directly.)
        let prs: Vec<serde_json::Value> =
            (1..=PAGE_SIZE as u64).map(|n| pr_json(n, &format!("fix(lsp): n#{n}"))).collect();
        let pages = single_page(prs);
        let outcome = assemble_outcome(pages, "owner/repo", true, "fixture")?;

        assert_eq!(outcome.receipt.fetched_unique, PAGE_SIZE);
        assert_eq!(outcome.receipt.pages_requested, 1);
        assert_eq!(outcome.receipt.completeness, Completeness::Truncated);
        Ok(())
    }

    #[test]
    fn test_assemble_outcome_201_prs_two_pages_complete() -> Result<()> {
        let page1: Vec<serde_json::Value> =
            (1..=100).map(|n| pr_json(n, &format!("fix(lsp): n#{n}"))).collect();
        let page2: Vec<serde_json::Value> =
            (101..=201).map(|n| pr_json(n, &format!("fix(lsp): n#{n}"))).collect();
        let pages = multi_page(vec![page1, page2]);
        let outcome = assemble_outcome(pages, "owner/repo", true, "fixture")?;

        assert_eq!(outcome.receipt.fetched_unique, 201);
        assert_eq!(outcome.receipt.pages_requested, 2);
        assert_eq!(outcome.receipt.completeness, Completeness::Complete);
        assert_eq!(outcome.receipt.ordered_identities.len(), 201);
        assert_eq!(
            outcome.receipt.page_identity,
            vec!["rest-page-0".to_string(), "rest-page-1".to_string()],
        );
        Ok(())
    }

    #[test]
    fn test_assemble_outcome_two_full_pages_is_truncated() -> Result<()> {
        let page1: Vec<serde_json::Value> =
            (1..=PAGE_SIZE as u64).map(|n| pr_json(n, &format!("fix(lsp): n#{n}"))).collect();
        let page2: Vec<serde_json::Value> =
            (101..=200).map(|n| pr_json(n, &format!("fix(lsp): n#{n}"))).collect();
        let pages = multi_page(vec![page1, page2]);
        let outcome = assemble_outcome(pages, "owner/repo", true, "fixture")?;

        assert_eq!(outcome.receipt.fetched_unique, 200);
        // The trailing page was exactly PAGE_SIZE, so we refuse completeness.
        assert_eq!(outcome.receipt.completeness, Completeness::Truncated);
        Ok(())
    }

    #[test]
    fn test_assemble_outcome_duplicate_across_pages_marks_snapshot_drift() -> Result<()> {
        // Even though GitHub's REST API should not repeat identities, we test
        // the defensive dedup path so a buggy or hostile API cannot silently
        // double-count rows.
        let page1 = vec![pr_json(1, "fix(lsp): a (#1)"), pr_json(2, "fix(lsp): b (#2)")];
        let page2 = vec![pr_json(2, "fix(lsp): b duplicate (#2)")];
        let pages = multi_page(vec![page1, page2]);
        let outcome = assemble_outcome(pages, "owner/repo", true, "fixture")?;

        assert_eq!(outcome.receipt.fetched_unique, 2);
        assert!(outcome.receipt.snapshot_drift);
        assert_eq!(outcome.receipt.completeness, Completeness::SnapshotDrift);
        // Ordering follows first appearance.
        assert_eq!(outcome.receipt.ordered_identities, vec![1, 2]);
        Ok(())
    }

    #[test]
    fn test_assemble_outcome_multi_page_digest_is_stable() -> Result<()> {
        let page1: Vec<serde_json::Value> =
            (1..=100).map(|n| pr_json(n, &format!("fix(lsp): n#{n}"))).collect();
        let page2: Vec<serde_json::Value> =
            (101..=150).map(|n| pr_json(n, &format!("fix(lsp): n#{n}"))).collect();
        let pages_a = multi_page(vec![page1.clone(), page2.clone()]);
        let pages_b = multi_page(vec![page1, page2]);
        let outcome_a = assemble_outcome(pages_a, "owner/repo", true, "fixture")?;
        let outcome_b = assemble_outcome(pages_b, "owner/repo", true, "fixture")?;

        assert_eq!(outcome_a.receipt.cursor_digest, outcome_b.receipt.cursor_digest);
        Ok(())
    }

    #[test]
    fn test_assemble_outcome_observed_at_varies_but_digest_stable() -> Result<()> {
        let prs = vec![pr_json(7, "fix(lsp): x (#7)")];
        let pages = single_page(prs);
        let a = assemble_outcome(
            single_page(vec![pr_json(7, "fix(lsp): x (#7)")]),
            "owner/repo",
            true,
            "fixture",
        )?;
        let b = assemble_outcome(pages, "owner/repo", false, "fixture")?;

        // observed_at is informational only; it does not affect the digest.
        assert_eq!(a.receipt.cursor_digest, b.receipt.cursor_digest);
        // The deterministic anchor is the Unix epoch; the live anchor is the
        // current time.
        assert_eq!(a.receipt.observed_at, "1970-01-01T00:00:00Z");
        assert_ne!(a.receipt.observed_at, b.receipt.observed_at);
        Ok(())
    }

    #[test]
    fn test_receipt_required_fields_are_present() -> Result<()> {
        let prs = vec![pr_json(42, "fix(lsp): receipt-fields (#42)")];
        let outcome = assemble_outcome(single_page(prs), "owner/repo", true, "fixture")?;

        assert_eq!(outcome.receipt.schema_version, SCHEMA_VERSION);
        assert_eq!(outcome.receipt.repository, "owner/repo");
        assert_eq!(outcome.receipt.query_state, "open");
        assert_eq!(outcome.receipt.api_generation, "v3");
        assert!(outcome.receipt.reported_total.is_none());
        assert_eq!(outcome.receipt.fetched_unique, 1);
        assert_eq!(outcome.receipt.pages_requested, 1);
        assert_eq!(outcome.receipt.page_size, PAGE_SIZE);
        assert_eq!(outcome.receipt.page_identity, vec!["rest-page-0".to_string()]);
        assert_eq!(outcome.receipt.ordered_identities, vec![42]);
        // 64 lowercase hex chars.
        assert_eq!(outcome.receipt.cursor_digest.len(), 64);
        assert!(
            outcome
                .receipt
                .cursor_digest
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        Ok(())
    }

    #[test]
    fn test_receipt_serializes_with_snake_case_enums() -> Result<()> {
        let prs = vec![pr_json(1, "fix(lsp): serialize (#1)")];
        let outcome = assemble_outcome(single_page(prs), "owner/repo", true, "fixture")?;

        let json = serde_json::to_string(&outcome.receipt)?;
        assert!(json.contains("\"completeness\":\"complete\""), "{json}");
        Ok(())
    }

    #[test]
    fn test_completeness_predicates() {
        assert!(Completeness::Complete.is_complete());
        assert!(!Completeness::Complete.is_partial());
        assert!(!Completeness::Truncated.is_complete());
        assert!(Completeness::Truncated.is_partial());
        assert!(!Completeness::SourceUnavailable.is_complete());
        assert!(Completeness::SourceUnavailable.is_partial());
        assert!(!Completeness::SnapshotDrift.is_complete());
        assert!(Completeness::SnapshotDrift.is_partial());
    }

    #[test]
    fn test_assemble_outcome_pages_over_max_is_an_error() {
        let empty_pages: Vec<Vec<serde_json::Value>> =
            (0..=MAX_PAGES_PER_REPO).map(|_| vec![]).collect();
        let pages = multi_page(empty_pages);
        let result = assemble_outcome(pages, "owner/repo", true, "fixture");
        assert!(result.is_err(), "expected max-pages error");
    }

    // ----- collect_pages stop discipline ------------------------------------

    #[test]
    fn test_collect_pages_stops_at_first_short_page() -> Result<()> {
        let pages = collect_pages(|page_number| {
            Ok(match page_number {
                1 => serde_json::json!(
                    (1..=PAGE_SIZE as u64).map(|n| pr_json(n, "x")).collect::<Vec<_>>()
                ),
                _ => serde_json::json!([]),
            })
        })?;

        // One full page plus the empty terminal probe: exact multiples are
        // complete, not truncated.
        assert_eq!(pages.len(), 2);
        let outcome =
            assemble_outcome(serde_json::Value::from(pages), "owner/repo", true, "gh api")?;
        assert_eq!(outcome.receipt.completeness, Completeness::Complete);
        assert_eq!(outcome.receipt.pages_requested, 2);
        Ok(())
    }

    #[test]
    fn test_collect_pages_stops_at_cap() -> Result<()> {
        let mut requested = 0u64;
        let pages = collect_pages(|page_number| {
            requested += 1;
            Ok(serde_json::json!(
                (1..=PAGE_SIZE as u64)
                    .map(|n| pr_json((page_number - 1) * PAGE_SIZE as u64 + n, "x"))
                    .collect::<Vec<_>>()
            ))
        })?;

        // The loop stops instead of paging forever; assembly reports the
        // full trailing page as truncated.
        assert_eq!(pages.len(), MAX_PAGES_PER_REPO);
        assert_eq!(requested, MAX_PAGES_PER_REPO as u64);
        let outcome =
            assemble_outcome(serde_json::Value::from(pages), "owner/repo", true, "gh api")?;
        assert_eq!(outcome.receipt.completeness, Completeness::Truncated);
        Ok(())
    }

    #[test]
    fn test_collect_pages_short_first_page_is_complete() -> Result<()> {
        let pages = collect_pages(|_| Ok(serde_json::json!([pr_json(1, "x")])))?;

        assert_eq!(pages.len(), 1);
        let outcome =
            assemble_outcome(serde_json::Value::from(pages), "owner/repo", true, "gh api")?;
        assert_eq!(outcome.receipt.completeness, Completeness::Complete);
        Ok(())
    }

    // ----- fixture-based end-to-end -----------------------------------------

    #[test]
    fn test_generate_from_fixture_writes_receipt() -> Result<()> {
        use std::io::Write;

        let tmp = tempfile::tempdir().context("creating temp dir")?;

        // Write a small canned gh JSON fixture in the REST list shape.
        let fixture_data = serde_json::json!([
            {
                "number": 101,
                "title": "feat(lsp): add definition provider (#101)",
                "labels": [{"name": "size/S"}],
                "draft": false,
                "head": {"ref": "feat/101-def-provider"},
                "user": {"login": "EffortlessSteven"}
            },
            {
                "number": 102,
                "title": "fix(parser): heredoc edge case (#102)",
                "labels": [{"name": "area/parser"}],
                "draft": true,
                "head": {"ref": "fix/102-heredoc"},
                "user": {"login": "bot"}
            }
        ]);

        let fixture_path = tmp.path().join("fixture.json");
        let mut f = fs::File::create(&fixture_path).context("creating fixture file")?;
        write!(f, "{}", serde_json::to_string_pretty(&fixture_data)?)?;

        let out_dir = tmp.path().join("out");

        generate(GenerateConfig {
            repos: vec!["EffortlessMetrics/perl-lsp-swarm".to_string()],
            out: out_dir.clone(),
            fixture: Some(fixture_path),
            paginated_fixture: None,
            deterministic_clock: true,
        })?;

        // Check repo JSON was written.
        let repo_json_path = out_dir.join("EffortlessMetrics-perl-lsp-swarm.json");
        assert!(repo_json_path.exists(), "repo JSON not found");

        let content = fs::read_to_string(&repo_json_path)?;
        let rows: Vec<LedgerRow> = serde_json::from_str(&content)?;

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].pr, "101");
        assert_eq!(rows[0].classification, "unclassified");
        assert_eq!(rows[0].surface_guess, "lsp");
        assert!(!rows[0].is_draft);

        assert_eq!(rows[1].pr, "102");
        assert_eq!(rows[1].surface_guess, "parser"); // label wins
        assert!(rows[1].is_draft);

        // Check receipt JSON was written.
        let receipt_path = out_dir.join("EffortlessMetrics-perl-lsp-swarm.receipt.json");
        assert!(receipt_path.exists(), "receipt JSON not found");
        let receipt: PaginationReceipt = serde_json::from_str(&fs::read_to_string(&receipt_path)?)?;
        assert_eq!(receipt.fetched_unique, 2);
        assert_eq!(receipt.pages_requested, 1);
        assert_eq!(receipt.completeness, Completeness::Complete);
        assert_eq!(receipt.ordered_identities, vec![101, 102]);

        // Check summary markdown was written.
        let md_path = out_dir.join("pr-ledger.md");
        assert!(md_path.exists(), "summary markdown not found");
        let md = fs::read_to_string(&md_path)?;
        assert!(md.contains("EffortlessMetrics/perl-lsp-swarm"));
        assert!(md.contains("#101"));
        assert!(md.contains("#102"));
        // The summary carries the receipt completeness state per repo.
        assert!(md.contains("*Inventory: complete (2 PRs;"), "{md}");

        Ok(())
    }

    #[test]
    fn test_generate_paginated_fixture_with_201_rows() -> Result<()> {
        use std::io::Write;

        let tmp = tempfile::tempdir().context("creating temp dir")?;

        let mut pages: Vec<Vec<serde_json::Value>> = Vec::new();
        pages.push(
            (1..=100)
                .map(|n| {
                    serde_json::json!({
                        "number": n,
                        "title": format!("fix(lsp): page1-{n} (#{n})"),
                        "labels": [],
                        "draft": false,
                        "head": {"ref": format!("feat/{n}-page1")},
                        "user": {"login": "test-user"}
                    })
                })
                .collect(),
        );
        pages.push(
            (101..=201)
                .map(|n| {
                    serde_json::json!({
                        "number": n,
                        "title": format!("fix(lsp): page2-{n} (#{n})"),
                        "labels": [],
                        "draft": false,
                        "head": {"ref": format!("feat/{n}-page2")},
                        "user": {"login": "test-user"}
                    })
                })
                .collect(),
        );

        let fixture_path = tmp.path().join("paginated.json");
        let mut f = fs::File::create(&fixture_path).context("creating fixture file")?;
        write!(f, "{}", serde_json::to_string(&pages)?)?;

        let out_dir = tmp.path().join("out");

        generate(GenerateConfig {
            repos: vec!["owner/repo".to_string()],
            out: out_dir.clone(),
            fixture: None,
            paginated_fixture: Some(fixture_path),
            deterministic_clock: true,
        })?;

        let receipt_path = out_dir.join("owner-repo.receipt.json");
        let receipt: PaginationReceipt = serde_json::from_str(&fs::read_to_string(&receipt_path)?)?;
        assert_eq!(receipt.fetched_unique, 201);
        assert_eq!(receipt.pages_requested, 2);
        assert_eq!(receipt.completeness, Completeness::Complete);
        assert_eq!(receipt.ordered_identities.len(), 201);

        // Run the generator a second time on the same canonical input.
        // The receipt's digest and ordered_identities must match exactly;
        // observed_at stays pinned because deterministic_clock is set.
        let out_dir2 = tmp.path().join("out2");
        generate(GenerateConfig {
            repos: vec!["owner/repo".to_string()],
            out: out_dir2.clone(),
            fixture: None,
            paginated_fixture: Some(out_dir.join("owner-repo.receipt.json")), // wrong type; should error
            deterministic_clock: true,
        })
        .ok();

        // Re-run with the original fixture to assert byte-equivalence.
        let fixture_path2 = tmp.path().join("paginated2.json");
        let mut f2 = fs::File::create(&fixture_path2)?;
        write!(f2, "{}", serde_json::to_string(&pages)?)?;

        let out_dir3 = tmp.path().join("out3");
        generate(GenerateConfig {
            repos: vec!["owner/repo".to_string()],
            out: out_dir3.clone(),
            fixture: None,
            paginated_fixture: Some(fixture_path2),
            deterministic_clock: true,
        })?;
        let receipt_a = fs::read_to_string(&receipt_path)?;
        let receipt_b = fs::read_to_string(out_dir3.join("owner-repo.receipt.json"))?;
        assert_eq!(receipt_a, receipt_b, "second generation must be byte-identical");

        Ok(())
    }

    #[test]
    fn test_generate_refuses_to_label_truncated_as_total() -> Result<()> {
        // The receipt is the canonical authority: a "truncated" inventory
        // cannot be presented downstream as a complete denominator. This test
        // asserts the receipt records `completeness = truncated` and the rows
        // are still produced so consumers can choose to refuse them.
        use std::io::Write;

        let tmp = tempfile::tempdir().context("creating temp dir")?;
        let pages = vec![
            (1..=PAGE_SIZE as u64)
                .map(|n| {
                    serde_json::json!({
                        "number": n,
                        "title": format!("fix(lsp): n#{n} (#{n})"),
                        "labels": [],
                        "draft": false,
                        "head": {"ref": format!("feat/{n}-x")},
                        "user": {"login": "test-user"}
                    })
                })
                .collect::<Vec<_>>(),
        ];

        let fixture_path = tmp.path().join("paginated.json");
        let mut f = fs::File::create(&fixture_path)?;
        write!(f, "{}", serde_json::to_string(&pages)?)?;

        let out_dir = tmp.path().join("out");
        generate(GenerateConfig {
            repos: vec!["owner/repo".to_string()],
            out: out_dir.clone(),
            fixture: None,
            paginated_fixture: Some(fixture_path),
            deterministic_clock: true,
        })?;

        let receipt: PaginationReceipt =
            serde_json::from_str(&fs::read_to_string(out_dir.join("owner-repo.receipt.json"))?)?;
        assert_eq!(receipt.completeness, Completeness::Truncated);
        assert_eq!(receipt.fetched_unique, PAGE_SIZE);
        // Rows still exist for inspection; consumers must check the receipt
        // before claiming "all" or "total".
        let rows: Vec<LedgerRow> =
            serde_json::from_str(&fs::read_to_string(out_dir.join("owner-repo.json"))?)?;
        assert_eq!(rows.len(), PAGE_SIZE);

        // The summary banner carries the truncated state, never a total.
        let md = fs::read_to_string(out_dir.join("pr-ledger.md"))?;
        assert!(md.contains("*Inventory: truncated"), "{md}");
        assert!(!md.contains("total"), "{md}");

        Ok(())
    }

    #[test]
    fn test_md_escape_pipes() -> Result<()> {
        let escaped = md_escape("title | with | pipes");
        assert!(!escaped.contains('|'));
        Ok(())
    }

    // ----- empty repo -------------------------------------------------------

    #[test]
    fn test_generate_empty_fixture() -> Result<()> {
        use std::io::Write;

        let tmp = tempfile::tempdir().context("creating temp dir")?;
        let fixture_path = tmp.path().join("empty.json");
        let mut f = fs::File::create(&fixture_path).context("creating fixture file")?;
        write!(f, "[]")?;

        let out_dir = tmp.path().join("out");
        generate(GenerateConfig {
            repos: vec!["owner/repo".to_string()],
            out: out_dir.clone(),
            fixture: Some(fixture_path),
            paginated_fixture: None,
            deterministic_clock: true,
        })?;

        let repo_json_path = out_dir.join("owner-repo.json");
        assert!(repo_json_path.exists());

        let content = fs::read_to_string(&repo_json_path)?;
        let rows: Vec<LedgerRow> = serde_json::from_str(&content)?;
        assert!(rows.is_empty());

        let md = fs::read_to_string(out_dir.join("pr-ledger.md"))?;
        assert!(md.contains("No open PRs"));

        let receipt: PaginationReceipt =
            serde_json::from_str(&fs::read_to_string(out_dir.join("owner-repo.receipt.json"))?)?;
        assert_eq!(receipt.fetched_unique, 0);
        assert_eq!(receipt.completeness, Completeness::Complete);

        Ok(())
    }
}
