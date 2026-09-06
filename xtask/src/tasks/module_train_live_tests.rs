//! Focused proof for the C03 read-only live frontier slice (#11627).
//!
//! Layers:
//! * read-only law: the single observation choke point rejects every
//!   non-read-only command shape before spawning, and the command inventory
//!   contains only observation commands (shift-left falsifier 18);
//! * identity block: exact machine-checkable parsing, markdown decoration,
//!   malformed blocks, agreement verdicts;
//! * pure classifier: the action law over synthetic facts — viable candidate
//!   before duplicate START, no ranking, controller STOP, blocked-never-START,
//!   stale-head non-transfer, merge-ready typed blockers, fan-in receipt gate,
//!   hard-dependency nonterminal WAIT, main-movement neutrality, dirty-unique
//!   work, one action per conflict surface;
//! * corpus fixture: the full normalization path over the pinned manifest,
//!   covering every corpus PR's expected action plus determinism (two runs
//!   byte-identical, candidate order permutation moves no byte, `observed_at`
//!   outside the semantic digest) and tamper detection (digest drift, stored
//!   action drift);
//! * instrument failures: permission/rate-limit/truncation/local-git failure
//!   states are `NOT_PROVEN`, never absence, never pass.

use super::*;
use color_eyre::eyre::Result;

const CORPUS_FIXTURE: &str = include_str!("../../tests/fixtures/module-train-live/raw-corpus.json");
const CLEAN_SURFACE_FIXTURE: &str =
    include_str!("../../tests/fixtures/module-train-live/raw-clean-surface.json");

fn raw_from_text(text: &str) -> Result<RawObservation> {
    Ok(serde_json::from_str(text)?)
}

fn loaded() -> Result<LoadedManifest> {
    load_manifest()
}

fn normalize_raw(raw: &RawObservation) -> Result<LiveSnapshot> {
    normalize(raw, &loaded()?)
}

fn normalize_text(text: &str) -> Result<LiveSnapshot> {
    normalize(&raw_from_text(text)?, &loaded()?)
}

fn node<'a>(snapshot: &'a LiveSnapshot, node_id: &str) -> Result<&'a NodeLive> {
    snapshot
        .semantic
        .nodes
        .iter()
        .find(|node| node.node_id == node_id)
        .ok_or_else(|| color_eyre::eyre::eyre!("snapshot must carry node {node_id}"))
}

fn facts_base() -> NodeFacts {
    NodeFacts {
        role: "implementation".to_string(),
        buildable: true,
        c02_state: "ready".to_string(),
        c02_reasons: Vec::new(),
        git_local_ok: true,
        github_ok: true,
        git_remote_ok: true,
        ..NodeFacts::default()
    }
}

fn open_candidate() -> CandidateView {
    CandidateView {
        number: 3001,
        draft: false,
        mergeable: "MERGEABLE".to_string(),
        review_decision: String::new(),
        has_reviews: false,
        checks_failed: false,
        checks_pending: false,
        merged_in_local_head: None,
        head_oid: "dddddddddddddddddddddddddddddddddddddddd".to_string(),
        ..CandidateView::default()
    }
}

// ---------------------------------------------------------------------------
// Read-only law (falsifier 18).
// ---------------------------------------------------------------------------

#[test]
fn observation_inventory_is_read_only() {
    for entry in observation_command_inventory() {
        let lowered = entry.to_ascii_lowercase();
        for forbidden in [
            " push",
            "merge --",
            " rebase",
            "commit",
            " close",
            " create",
            " edit",
            "delete",
            "write",
            "apply",
            "restore",
            "stash",
            "pr merge",
            "pr close",
            "pr edit",
            "pr create",
            "pr comment",
            "pr review",
            "issue",
            "label",
            "repo sync",
        ] {
            assert!(
                !lowered.contains(forbidden),
                "observation inventory entry {entry:?} looks mutative ({forbidden})"
            );
        }
    }
    assert!(observation_command_inventory().iter().any(|entry| entry.starts_with("git ")));
    assert!(observation_command_inventory().iter().any(|entry| entry.starts_with("gh ")));
    // `gh api` is a general-purpose HTTP client and cannot be blanket-trusted.
    // The only admitted shape is the gated GraphQL read: every other `gh api`
    // entry is mutative until proven otherwise.
    for entry in observation_command_inventory() {
        if entry.starts_with("gh api") {
            assert!(
                entry.starts_with("gh api graphql -f query="),
                "the only admitted gh api shape is the gated read-only GraphQL query, got {entry:?}"
            );
        }
    }
}

/// The list leg must not be weaker than the GraphQL leg it is now joined with:
/// a valid-but-not-a-list response is an instrument failure, never an empty
/// population. Includes the opposite-direction control, because a gate that
/// rejects everything would also pass the negative cases.
#[test]
fn a_non_array_pr_list_response_is_instrument_failure_not_absence() -> Result<()> {
    // Opposite direction first: an empty array is a real, usable observation
    // of zero PRs and must stay Ok.
    assert!(list_rows("[]", "open").map_err(|e| e.to_string()).is_ok());
    let rows = list_rows(r#"[{"number":1},{"number":2}]"#, "open")
        .map_err(|error| color_eyre::eyre::eyre!("a populated list must parse: {error}"))?;
    assert_eq!(rows.len(), 2);

    // A syntactically valid non-array root cannot establish absence. `gh`
    // returns an object for several non-list outcomes, and an empty open
    // window additionally reports `open_truncated == false`, so accepting one
    // of these would prove "no candidate exists" from a response that never
    // listed anything.
    for body in [
        r#"{"message":"Not Found","documentation_url":"..."}"#,
        r#"{}"#,
        r#""unexpected string""#,
        "null",
        "0",
        "false",
    ] {
        let error = list_rows(body, "open")
            .err()
            .ok_or_else(|| color_eyre::eyre::eyre!("{body} must fail the instrument"))?;
        assert!(
            error.to_string().contains("not the expected array"),
            "expected a shape failure for {body}, got: {error}"
        );
    }

    // Malformed JSON stays a failure too.
    assert!(list_rows("{ not json", "open").is_err());
    Ok(())
}

/// The read-only law for GraphQL lives in the document, not the HTTP verb:
/// `gh api graphql` is a POST either way.
#[test]
fn graphql_read_only_law_is_carried_by_the_document() {
    // The document this observer actually sends must pass its own gate.
    assert!(args_read_only(
        "gh",
        &[
            "api",
            "graphql",
            "-f",
            &format!("query={GH_REVIEW_GRAPHQL}"),
            "-F",
            "owner=EffortlessMetrics",
            "-F",
            "name=perl-lsp-swarm",
            "-F",
            "pr=14237",
        ],
    ));

    for rejected in [
        // A write operation, transported identically to a query.
        "query=mutation { addComment(input: {}) { clientMutationId } }",
        // A write smuggled in behind a legitimate-looking read.
        "query=query Read { viewer { login } } mutation Write { closePullRequest { id } }",
        "query=subscription Watch { x }",
        // No operation keyword at all.
        "query=",
        "query={ viewer { login } }",
        // Not a query field.
        "mutation=mutation { x }",
    ] {
        assert!(
            !args_read_only("gh", &["api", "graphql", "-f", rejected]),
            "expected rejection for {rejected:?}"
        );
    }

    // Flags outside the observation contract are rejected wholesale, and a
    // variable may never carry a second document.
    assert!(!args_read_only("gh", &["api", "graphql", "-X", "POST", "-f", "query=query A { b }"]));
    assert!(!args_read_only("gh", &["api", "graphql", "--input", "body.json"]));
    assert!(!args_read_only("gh", &["api", "graphql"]));
    assert!(!args_read_only(
        "gh",
        &["api", "graphql", "-f", "query=query A { b }", "-F", "x=mutation { y }"],
    ));
    // Exactly one document; a second `-f query=` is a rejected shape.
    assert!(!args_read_only(
        "gh",
        &["api", "graphql", "-f", "query=query A { b }", "-f", "query=query C { d }"],
    ));
    // A non-graphql api path stays rejected.
    assert!(!args_read_only("gh", &["api", "repos/x/y/pulls"]));

    // `gh` lifts `query` and `operationName` out of the variable map into the
    // top level of the request body, so they are not variables at all and must
    // not pass the inert-variable shape check.
    for reserved in ["query=Something", "operationName=Something"] {
        assert!(
            !args_read_only("gh", &["api", "graphql", "-f", "query=query A { b }", "-F", reserved]),
            "reserved top-level field {reserved:?} must not pass as an inert variable"
        );
    }
}

/// `explain` must not print a fact as observed and then summarize it as
/// unavailable in the same report.
#[test]
fn explain_unavailable_summary_matches_the_observed_facts() -> Result<()> {
    let snapshot = normalize_text(CORPUS_FIXTURE)?;
    let manifest = loaded()?;

    // E00A's sole candidate is PR 2002: its review sits on a superseded
    // commit (currency IS observed — the answer is "no") while its thread page
    // is truncated (resolution is genuinely unprovable). The summary must
    // separate the two rather than lumping both under "unavailable".
    let mixed = render_explain(&snapshot, &manifest, "E00A")?;
    assert!(mixed.contains("reviewed_commit_is_head: no"), "{mixed}");
    assert!(mixed.contains("truncated=true"), "{mixed}");
    assert!(
        !mixed.contains("reviewed-commit comparison"),
        "the comparison succeeded here and must not be summarized as unavailable: {mixed}"
    );
    assert!(
        mixed.contains("review threads"),
        "a truncated thread page is genuinely unavailable and must be named: {mixed}"
    );
    // Semantic currency is unconditionally unavailable, comparison or not.
    assert!(mixed.contains("review-head currency"), "{mixed}");
    // Behavior receipts have no producer (#11619) and stay unconditional.
    assert!(mixed.contains("behavior receipts"), "{mixed}");

    // E00C holds two candidates: 2004 is fully observed, 2003 has no review
    // instrument at all. The node-level summary takes the weaker candidate, so
    // both facts are named — conservative, and not a contradiction with 2004's
    // own observed lines above it.
    let mixed_candidates = render_explain(&snapshot, &manifest, "E00C")?;
    assert!(mixed_candidates.contains("reviewed_commit_is_head: yes"), "{mixed_candidates}");
    assert!(mixed_candidates.contains("reviewed-commit comparison"), "{mixed_candidates}");
    assert!(mixed_candidates.contains("review threads"), "{mixed_candidates}");
    Ok(())
}

/// Review facts are fetched after the PR list, so the head they describe must
/// be the head the list reported or they bind to nothing.
#[test]
fn review_facts_do_not_bind_across_a_moved_head() {
    let listed = "a".repeat(40);
    let moved = "b".repeat(40);

    assert!(review_facts_bind_to_listed_head(&listed, &listed));
    // A push landed between the list read and the review read.
    assert!(!review_facts_bind_to_listed_head(&listed, &moved));
    // An unusable oid on either side binds nothing.
    assert!(!review_facts_bind_to_listed_head("", &listed));
    assert!(!review_facts_bind_to_listed_head(&listed, ""));
    assert!(!review_facts_bind_to_listed_head("", ""));
}

/// A review page that did not cover every review cannot prove currency: the
/// omitted review is exactly the one that might be stale.
#[test]
fn truncated_review_page_cannot_prove_currency() {
    let head = "a".repeat(40);
    // Every *observed* review is on the head, but the page was incomplete.
    assert_eq!(
        review_commit_matches_head(&head, &[review_at(Some(&head)), review_at(Some(&head))], true),
        None,
        "an incomplete review page must never report currency"
    );
    // The same observation with a complete page does prove it.
    assert_eq!(
        review_commit_matches_head(&head, &[review_at(Some(&head)), review_at(Some(&head))], false),
        Some(true)
    );
}

#[test]
fn non_read_only_commands_are_rejected_before_spawning() -> Result<()> {
    for (program, args) in [
        ("git", vec!["push", "origin", "main"]),
        ("git", vec!["merge", "--squash", "main"]),
        ("git", vec!["commit", "-am", "x"]),
        ("git", vec!["worktree", "add", "../x"]),
        ("git", vec!["worktree", "remove", "../x"]),
        ("git", vec!["remote", "add", "upstream", "https://example.invalid"]),
        ("git", vec!["rebase", "origin/main"]),
        ("git", vec!["stash"]),
        ("gh", vec!["pr", "merge", "3001"]),
        ("gh", vec!["pr", "close", "3001"]),
        ("gh", vec!["pr", "create", "--title", "x"]),
        ("gh", vec!["pr", "edit", "3001"]),
        ("gh", vec!["pr", "comment", "3001"]),
        ("gh", vec!["pr", "review", "3001", "--approve"]),
        ("gh", vec!["issue", "close", "11627"]),
        ("gh", vec!["api", "-X", "POST", "repos/x/y/pulls"]),
        ("gh", vec!["api", "graphql", "-f", "query=mutation { closePullRequest { id } }"]),
        ("gh", vec!["api", "graphql", "-X", "POST", "-f", "query=query A { b }"]),
        ("curl", vec!["https://example.invalid"]),
    ] {
        let refusal = run_observation(None, program, &args)
            .err()
            .ok_or_else(|| color_eyre::eyre::eyre!("mutative command shapes must be rejected"))?;
        assert!(
            refusal.to_string().contains("rejected non-read-only"),
            "expected a rejection before spawning for {program} {args:?}, got {refusal}"
        );
    }
    Ok(())
}

#[test]
fn read_only_shapes_pass_the_gate() {
    assert!(args_read_only("git", &["rev-parse", "HEAD"]));
    assert!(args_read_only("git", &["status", "--porcelain"]));
    assert!(args_read_only("git", &["for-each-ref", "refs/heads/"]));
    assert!(args_read_only("git", &["ls-remote", "origin", "refs/heads/*"]));
    assert!(args_read_only("git", &["merge-base", "--is-ancestor", "a", "HEAD"]));
    assert!(args_read_only("git", &["worktree", "list", "--porcelain"]));
    assert!(args_read_only("git", &["remote", "get-url", "origin"]));
    assert!(args_read_only("gh", &["pr", "list", "--state", "open"]));
    assert!(args_read_only("gh", &["pr", "view", "3001", "--json", "number"]));
    assert!(!args_read_only("gh", &["pr", "merge", "3001"]));
}

// ---------------------------------------------------------------------------
// Identity-block law.
// ---------------------------------------------------------------------------

#[test]
fn identity_block_parses_exact_and_decorated_forms() {
    let exact = "Module train: #11625\nModule node: #8497\nParent/controller: #4240\n";
    assert_eq!(
        parse_identity_block(exact),
        Some(IdentityBlock { train_issue: 11625, node_issue: 8497, controller_issue: 4240 })
    );
    let decorated = "prose\n\n- **Module train:** `#11625`\n- **Module node:** `#8497`\n- **Parent/controller:** `#4240`\n";
    assert!(parse_identity_block(decorated).is_some(), "decorated forms must parse");
}

#[test]
fn identity_block_rejects_malformed_and_missing_parts() {
    assert!(parse_identity_block("no block at all").is_none());
    assert!(parse_identity_block("Module train: #11625\nModule node: #8497").is_none());
    assert!(
        parse_identity_block("Module train: 11625\nModule node: #8497\nParent/controller: #4240")
            .is_none(),
        "values must be explicit #issue references"
    );
    assert!(
        parse_identity_block("Module train: #abc\nModule node: #8497\nParent/controller: #4240")
            .is_none()
    );
    // Prose mentions are never a block.
    assert!(parse_identity_block("works on 8497 and 11625 but carries no keys").is_none());
}

#[test]
fn title_similarity_and_prose_mentions_never_bind() -> Result<()> {
    let snapshot = normalize_text(CORPUS_FIXTURE)?;
    // PR #2011 mentions 8497/11627 in prose; it must not be stored at all.
    assert!(
        snapshot.semantic.github.prs.iter().all(|pr| pr.number != 2011),
        "unrelated PRs must be dropped from the bounded snapshot"
    );
    // M01's bound candidate is exactly #2001 (via its identity block).
    let m01 = node(&snapshot, "M01")?;
    assert_eq!(m01.candidates.len(), 1);
    assert_eq!(m01.candidates[0].number, 2001);
    Ok(())
}

// ---------------------------------------------------------------------------
// Pure classifier: action laws (falsifiers 1-5, 8-14, 16, 17).
// ---------------------------------------------------------------------------

#[test]
fn viable_candidate_blocks_duplicate_start() {
    // Falsifier 1: a viable canonical candidate exists; START must not win.
    let mut facts = facts_base();
    facts.open_bound = vec![open_candidate()];
    let classified = classify(&facts);
    assert_eq!(classified.action, Action::Review);
    assert!(classified.flags.contains(&"canonical_candidate".to_string()));
}

#[test]
fn two_candidates_reconcile_without_ranking() {
    // Falsifier 2: the newer/greener/approved candidate must not win.
    let mut facts = facts_base();
    let mut older = open_candidate();
    older.number = 3000;
    older.review_decision = String::new();
    let mut newer = open_candidate();
    newer.number = 3002;
    newer.review_decision = "APPROVED".to_string();
    newer.head_oid = "9999999999999999999999999999999999999999".to_string();
    facts.open_bound = vec![older, newer];
    let classified = classify(&facts);
    assert_eq!(classified.action, Action::Reconcile);
    assert!(classified.flags.contains(&"multiple_candidates".to_string()));
    // Rank-independent: swapping order changes nothing.
    facts.open_bound.reverse();
    let swapped = classify(&facts);
    assert_eq!(swapped, classified);
    // Same head bound twice additionally flags a duplicate.
    let twin = open_candidate();
    facts.open_bound = vec![twin.clone(), twin];
    let duplicate = classify(&facts);
    assert!(duplicate.flags.contains(&"duplicate_candidate".to_string()));
}

#[test]
fn controller_bound_as_implementation_stops() {
    // Falsifier 3.
    let mut facts = facts_base();
    facts.role = "controller".to_string();
    facts.open_bound = vec![open_candidate()];
    let classified = classify(&facts);
    assert_eq!(classified.action, Action::Stop);
    assert!(classified.flags.contains(&"controller_candidate".to_string()));
}

#[test]
fn static_blocked_leaf_never_starts_for_absence_of_a_pr() {
    // Falsifier 4.
    let mut facts = facts_base();
    facts.c02_state = "blocked_hard".to_string();
    facts.c02_reasons = vec!["hard_dep_not_landed:C02".to_string()];
    let classified = classify(&facts);
    assert_eq!(classified.action, Action::Blocked);
    assert!(classified.reasons.contains(&"hard_dep_not_landed:C02".to_string()));
}

#[test]
fn checks_or_review_on_a_moved_head_never_transfer() {
    // Falsifier 5: review facts on a previous head cannot satisfy the moved
    // head; approval facts are only usable when review_on_head is exact.
    let mut facts = facts_base();
    let mut candidate = open_candidate();
    candidate.review_decision = "APPROVED".to_string();
    candidate.has_reviews = true;
    candidate.review_on_head = Some(false);
    facts.open_bound = vec![candidate];
    let classified = classify(&facts);
    assert_eq!(classified.action, Action::NotProven);
    assert!(classified.flags.contains(&"head_moved_after_review".to_string()));
    assert!(classified.reasons.contains(&"review_not_on_current_head".to_string()));
}

#[test]
fn wrong_base_or_malformed_stack_reconciles() {
    // Falsifier 8 (facts-level law; corpus covers the binding verdict).
    let mut facts = facts_base();
    facts.misbound_refs = vec![MisboundRef {
        number: 3003,
        reasons: vec!["wrong_dependency_or_stack_relation:base=tooling/other".to_string()],
    }];
    let classified = classify(&facts);
    assert_eq!(classified.action, Action::Reconcile);
    assert!(classified.flags.contains(&"wrong_dependency_or_stack_relation".to_string()));
    assert!(classified.reasons.contains(&"misbound_candidate_pr:#3003".to_string()));
}

#[test]
fn dirty_or_unpushed_unique_work_is_never_disposable() {
    // Falsifier 9.
    let mut facts = facts_base();
    facts.surfaces = vec![SurfaceView {
        kind: "local_branch".to_string(),
        name: "wip/10573-context-contract".to_string(),
        dirty: true,
        unpushed: true,
    }];
    let classified = classify(&facts);
    assert_eq!(classified.action, Action::Reconcile);
    assert!(classified.flags.contains(&"dirty_or_unpushed_unique_work".to_string()));
}

#[test]
fn one_action_per_conflict_surface() -> Result<()> {
    // Falsifier 10: classification emits one action per node; the corpus
    // snapshot's conflict-key map is duplicate-free (asserted in normalize and
    // re-asserted here over the stored actions).
    let snapshot = normalize_text(CORPUS_FIXTURE)?;
    let mut keys = std::collections::BTreeSet::new();
    for node in &snapshot.semantic.nodes {
        assert!(keys.insert(node.conflict_key.as_str()), "duplicate conflict key");
        assert!(
            Action::from_str(&node.action).is_some(),
            "node {} action {} must be in the closed vocabulary",
            node.node_id,
            node.action
        );
    }
    Ok(())
}

#[test]
fn dependent_waits_while_a_hard_dep_candidate_is_nonterminal() {
    // Falsifier 11 (L09G-class law).
    let mut facts = facts_base();
    facts.hard_dep_nonterminal = vec!["L09A".to_string(), "L09B".to_string()];
    let classified = classify(&facts);
    assert_eq!(classified.action, Action::Wait);
    assert!(classified.reasons.contains(&"hard_dep_candidate_nonterminal:L09A".to_string()));
    assert!(classified.reasons.contains(&"hard_dep_candidate_nonterminal:L09B".to_string()));
}

#[test]
fn fan_in_cannot_start_without_child_receipts() {
    // Falsifier 12 (P11F-class law): receipts are unobservable -> fail closed.
    let mut facts = facts_base();
    facts.role = "fan_in".to_string();
    let classified = classify(&facts);
    assert_eq!(classified.action, Action::NotProven);
    assert!(classified.reasons.contains(&"child_receipts_not_observable".to_string()));
    assert!(classified.limitations.contains(&"behavior_receipts_not_observable".to_string()));
}

#[test]
fn core_receipt_cannot_hide_edit_profile_non_pass() {
    // Falsifier 13.
    let mut facts = facts_base();
    let mut candidate = open_candidate();
    candidate.review_decision = "APPROVED".to_string();
    candidate.has_reviews = true;
    candidate.review_on_head = Some(true);
    candidate.threads_resolved = Some(true);
    candidate.core_receipt_pass = Some(true);
    candidate.edit_profile_pass = Some(false);
    facts.open_bound = vec![candidate];
    let classified = classify(&facts);
    assert_eq!(classified.action, Action::NotProven);
    assert!(
        classified.reasons.contains(&"core_receipt_cannot_hide_edit_profile_non_pass".to_string())
    );
}

#[test]
fn exact_process_receipt_is_not_broader_support_truth() {
    // Falsifier 14.
    let mut facts = facts_base();
    let mut candidate = open_candidate();
    candidate.review_decision = "APPROVED".to_string();
    candidate.has_reviews = true;
    candidate.review_on_head = Some(true);
    candidate.threads_resolved = Some(true);
    candidate.exact_process_receipt_pass = Some(true);
    candidate.edit_profile_pass = Some(false);
    facts.open_bound = vec![candidate];
    let classified = classify(&facts);
    assert_eq!(classified.action, Action::NotProven);
    assert!(
        classified
            .reasons
            .contains(&"exact_process_receipt_is_not_broader_support_truth".to_string())
    );
}

#[test]
fn merge_ready_requires_threads_receipts_and_currency() {
    // The positive branch: complete synthetic facts DO reach the
    // recommendation (proving the branch exists), and each missing fact
    // blocks it (falsifier 16).
    let mut facts = facts_base();
    let mut candidate = open_candidate();
    candidate.review_decision = "APPROVED".to_string();
    candidate.has_reviews = true;
    candidate.review_on_head = Some(true);
    candidate.threads_resolved = Some(true);
    candidate.core_receipt_pass = Some(true);
    candidate.edit_profile_pass = Some(true);
    facts.open_bound = vec![candidate.clone()];
    assert_eq!(classify(&facts).action, Action::MergeReadyRecommendation);

    candidate.threads_resolved = Some(false);
    facts.open_bound = vec![candidate];
    let classified = classify(&facts);
    assert_eq!(classified.action, Action::NotProven);
    assert!(classified.reasons.contains(&"review_threads_unresolved".to_string()));
}

fn review_at(commit: Option<&str>) -> ReviewFacts {
    ReviewFacts {
        author_login: "reviewer".to_string(),
        state: "APPROVED".to_string(),
        submitted_at: Some("2026-08-30T00:00:00Z".to_string()),
        commit_oid: commit.map(str::to_string),
    }
}

/// The commit comparison is exact and fail-closed. It is a diagnostic: the
/// classifier must never turn it into a currency verdict (see
/// `semantic_currency_is_never_derived_from_a_head_sha`).
#[test]
fn reviewed_commit_comparison_is_bound_to_the_observed_commit() {
    let head = "a".repeat(40);
    let stale = "b".repeat(40);

    assert_eq!(review_commit_matches_head(&head, &[review_at(Some(&head))], false), Some(true));
    // The head moved after the review was submitted: definitively not current.
    assert_eq!(review_commit_matches_head(&head, &[review_at(Some(&stale))], false), Some(false));
    // One stale review among current ones still blocks currency.
    assert_eq!(
        review_commit_matches_head(
            &head,
            &[review_at(Some(&head)), review_at(Some(&stale))],
            false
        ),
        Some(false)
    );
    // Unbindable inputs stay unprovable — never Some(true), and never
    // Some(false) either: "cannot tell" must not raise head_moved_after_review.
    assert_eq!(review_commit_matches_head(&head, &[review_at(None)], false), None);
    assert_eq!(review_commit_matches_head(&head, &[review_at(Some(""))], false), None);
    assert_eq!(review_commit_matches_head("", &[review_at(Some(&head))], false), None);
    assert_eq!(review_commit_matches_head(&head, &[], false), None);
    // An incomplete review page cannot prove currency: the omitted review is
    // exactly the one that might be stale.
    assert_eq!(review_commit_matches_head(&head, &[review_at(Some(&head))], true), None);
}

/// An unobserved or truncated thread page can never read as resolved.
#[test]
fn thread_resolution_never_passes_on_partial_observation() {
    let observed = |total: usize, unresolved: usize, truncated: bool| ReviewThreadFacts {
        observed: true,
        total,
        unresolved,
        truncated,
    };

    assert_eq!(threads_resolved(&observed(3, 0, false)), Some(true));
    assert_eq!(threads_resolved(&observed(3, 1, false)), Some(false));
    // Zero threads observed is a real "nothing unresolved".
    assert_eq!(threads_resolved(&observed(0, 0, false)), Some(true));
    // Truncated: no unresolved thread was *seen*, which is not the same as
    // none existing.
    assert_eq!(threads_resolved(&observed(500, 0, true)), None);
    // No instrument ran at all.
    assert_eq!(threads_resolved(&ReviewThreadFacts::default()), None);
}

/// The typed blockers must name only what is actually unobservable. Behavior
/// receipts have no producer in this tree (#11619) and stay blocking; thread
/// resolution stops being claimed as unobservable once it is observed.
#[test]
fn observed_thread_resolution_stops_being_reported_as_a_blocker() {
    let mut facts = facts_base();
    let mut candidate = open_candidate();
    candidate.review_decision = "APPROVED".to_string();
    candidate.has_reviews = true;
    candidate.threads_resolved = Some(true);
    facts.open_bound = vec![candidate.clone()];

    let classified = classify(&facts);
    // Still not merge-ready: receipts remain a real blocker.
    assert_eq!(classified.action, Action::NotProven);
    assert!(
        classified.limitations.contains(&"behavior_receipts_not_observable".to_string()),
        "receipts have no producer yet and must stay a typed blocker"
    );
    assert!(!classified.limitations.contains(&"review_threads_not_observable".to_string()));

    // When the thread instrument genuinely could not bind it, the blocker
    // comes back.
    candidate.threads_resolved = None;
    facts.open_bound = vec![candidate];
    let classified = classify(&facts);
    assert!(classified.limitations.contains(&"review_threads_not_observable".to_string()));
}

/// The repository's currentness authority is explicit that a head SHA is not a
/// review-validity token: `docs/agents/REVIEW_CURRENTNESS.md` ("Review is
/// semantic, not exact-head", "A SHA change by itself appears nowhere in this
/// table") and `AGENTS.md` ("head SHA change alone -> no review invalidation").
///
/// So a differing reviewed commit is reported as a diagnostic and never as an
/// invalidated review, and semantic currency stays a typed blocker.
#[test]
fn semantic_currency_is_never_derived_from_a_head_sha() {
    let mut facts = facts_base();
    let mut candidate = open_candidate();
    candidate.has_reviews = true;
    // The reviewed commit is not the head — e.g. a later formatting-only push.
    candidate.reviewed_commit_is_head = Some(false);
    candidate.threads_resolved = Some(true);
    facts.open_bound = vec![candidate.clone()];

    let classified = classify(&facts);
    assert_eq!(classified.action, Action::Review);
    // The diagnostic is reported...
    assert!(classified.reasons.contains(&"reviewed_commit_differs_from_head".to_string()));
    // ...but it must never assert that the review was invalidated.
    assert!(
        !classified.flags.contains(&"head_moved_after_review".to_string()),
        "a SHA delta alone must not assert review invalidation: {:?}",
        classified.flags
    );
    // Semantic currency remains unobservable regardless of the comparison.
    assert!(classified.limitations.contains(&"review_head_currency_not_observable".to_string()));

    // The same holds when the reviewed commit IS the head: matching SHAs do
    // not prove the review is semantically current either.
    candidate.reviewed_commit_is_head = Some(true);
    facts.open_bound = vec![candidate];
    let classified = classify(&facts);
    assert!(
        classified.limitations.contains(&"review_head_currency_not_observable".to_string()),
        "a matching SHA must not manufacture currency: {:?}",
        classified.limitations
    );
    assert!(!classified.reasons.contains(&"reviewed_commit_differs_from_head".to_string()));
}

/// End-to-end: the observed review facts survive raw -> snapshot normalization
/// and derive the same way the classifier consumes them.
#[test]
fn corpus_carries_observed_review_facts_through_normalization() -> Result<()> {
    let snapshot = normalize_text(CORPUS_FIXTURE)?;
    let pr = |number: u64| {
        snapshot
            .semantic
            .github
            .prs
            .iter()
            .find(|pr| pr.number == number)
            .unwrap_or_else(|| panic!("fixture PR {number} must normalize"))
    };

    // 2004: review bound to the observed head, every thread observed+resolved.
    let approved = pr(2004);
    assert_eq!(approved.latest_reviews[0].commit_oid.as_deref(), Some(approved.head_oid.as_str()));
    assert!(approved.review_threads.observed);
    assert!(!approved.review_threads.truncated);
    assert_eq!(
        review_commit_matches_head(
            &approved.head_oid,
            &approved.latest_reviews,
            approved.review_page_truncated
        ),
        Some(true)
    );
    assert_eq!(threads_resolved(&approved.review_threads), Some(true));

    // 2002: review left on a superseded commit, thread page truncated.
    let stale = pr(2002);
    assert_ne!(stale.latest_reviews[0].commit_oid.as_deref(), Some(stale.head_oid.as_str()));
    assert_eq!(
        review_commit_matches_head(
            &stale.head_oid,
            &stale.latest_reviews,
            stale.review_page_truncated
        ),
        Some(false)
    );
    assert!(stale.review_threads.truncated);
    assert_eq!(
        threads_resolved(&stale.review_threads),
        None,
        "a truncated thread page must never resolve"
    );

    // The production constructor is where currency could most easily be
    // re-derived from the commit comparison by accident, so pin it directly:
    // #2001's review IS on its head, and the currency input must still be
    // None. A matching SHA is not semantic currency.
    let view = candidate_view(pr(2001));
    assert_eq!(
        view.reviewed_commit_is_head,
        Some(true),
        "the diagnostic comparison must still be reported"
    );
    assert_eq!(
        view.review_on_head, None,
        "candidate_view must never derive semantic currency from a head SHA"
    );

    // The REVIEW route through the whole pipeline: M01's candidate #2001 has
    // an opinionated review bound to its head and every thread resolved. The
    // observed thread resolution must reach the classifier, while review-head
    // currency stays a typed blocker even though the SHAs match — matching
    // commits do not manufacture semantic currency.
    let m01 = node(&snapshot, "M01")?;
    assert_eq!(m01.action, "REVIEW");
    assert!(
        !m01.limitations.iter().any(|limitation| limitation == "review_threads_not_observable"),
        "observed thread resolution must not be reported as a blocker: {:?}",
        m01.limitations
    );
    assert!(
        m01.limitations
            .iter()
            .any(|limitation| limitation == "review_head_currency_not_observable"),
        "semantic currency is never derivable from a head SHA: {:?}",
        m01.limitations
    );
    assert!(
        !m01.action_reasons.iter().any(|reason| reason == "reviewed_commit_differs_from_head"),
        "this candidate's review IS on the head commit: {:?}",
        m01.action_reasons
    );

    // A candidate with no review instrument at all keeps both facts unobserved.
    let unobserved = pr(2003);
    assert!(!unobserved.review_threads.observed);
    assert_eq!(threads_resolved(&unobserved.review_threads), None);
    assert_eq!(
        review_commit_matches_head(
            &unobserved.head_oid,
            &unobserved.latest_reviews,
            unobserved.review_page_truncated
        ),
        None
    );
    Ok(())
}

#[test]
fn main_movement_alone_changes_no_action() -> Result<()> {
    // Falsifier 17: classification consumes no main-SHA input, so unrelated
    // main movement cannot invalidate or promote an action.
    let mut raw = raw_from_text(CORPUS_FIXTURE)?;
    let first = normalize_raw(&raw)?;
    raw.repository.observed_main_sha = Some("ffffffffffffffffffffffffffffffffffffffff".to_string());
    let second = normalize_raw(&raw)?;
    let actions_first: Vec<(&String, &String)> =
        first.semantic.nodes.iter().map(|n| (&n.node_id, &n.action)).collect();
    let actions_second: Vec<(&String, &String)> =
        second.semantic.nodes.iter().map(|n| (&n.node_id, &n.action)).collect();
    assert_eq!(actions_first, actions_second, "main movement must not move actions");
    Ok(())
}

// ---------------------------------------------------------------------------
// Corpus: full normalization over the pinned manifest.
// ---------------------------------------------------------------------------

#[test]
fn corpus_classifies_every_expected_action() -> Result<()> {
    let snapshot = normalize_text(CORPUS_FIXTURE)?;
    let expect = [
        ("C01", "WAIT", "landed_current_tree_no_writer_action"),
        ("C02", "WAIT", "landed_current_tree_no_writer_action"),
        ("C03", "BLOCKED", "hard_dep_not_landed:C02"),
        ("CTRL", "STOP", "controller_selected_as_implementation"),
        ("E00A", "REPAIR", "review_changes_requested"),
        ("E00C", "RECONCILE", "multiple_bound_candidates_need_bounded_ownership_decision"),
        ("M01", "REVIEW", "review_head_currency_not_proven"),
        ("M07A", "RECONCILE", "unique_work_surface:local_branch:wip/10573-context-contract"),
        ("M07B", "RECONCILE", "closed_candidate_unique_work_needs_salvage_decision"),
        ("M07C", "RECONCILE", "binding_agreement_failed_needs_bounded_ownership_decision"),
        ("L09A", "WAIT", "merge_commit_not_ancestor_of_observed_head"),
    ];
    for (node_id, action, reason) in expect {
        let node = node(&snapshot, node_id)?;
        assert_eq!(node.action, action, "node {node_id} action");
        assert!(
            node.action_reasons.iter().any(|candidate| candidate == reason),
            "node {node_id} must carry reason {reason}, got {:?}",
            node.action_reasons
        );
    }
    // Falsifier 2 corpus half: the newer, approved, greener E00C candidate
    // (#2004) did not win over #2003; both are RECONCILE material only.
    let e00c = node(&snapshot, "E00C")?;
    assert_eq!(e00c.candidates.len(), 2);
    assert!(e00c.candidate_flags.contains(&"multiple_candidates".to_string()));
    // Falsifier 6: the merged-not-in-tree PR is pending probe, never landed.
    let l09a = node(&snapshot, "L09A")?;
    assert!(
        l09a.candidate_flags.contains(&"merged_candidate_pending_current_tree_probe".to_string()),
        "merged-but-absent commit must stay pending-probe"
    );
    assert!(node(&snapshot, "C02")?.candidate_flags.contains(&"merged_current_tree".to_string()));
    // Falsifier 7: stray issue closure/labels changed nothing (M01 still
    // classified from the train + candidate facts; C03 still BLOCKED).
    let m01 = node(&snapshot, "M01")?;
    assert_eq!(m01.action, "REVIEW");
    assert!(node(&snapshot, "C03")?.action == "BLOCKED");
    // Surfaces are diagnostics that never outvote the candidate: M01 keeps its
    // remote surface while its action stays REVIEW.
    assert!(m01.surfaces.iter().any(|surface| surface.kind == "remote_branch"));
    // Misbound PRs are recorded, never silently dropped.
    assert!(
        snapshot
            .semantic
            .github
            .misbound_prs
            .iter()
            .any(|pr| pr.number == 2009 && pr.node_id.as_deref() == Some("M07C")),
        "wrong-base PR must be recorded as misbound against its named node"
    );
    assert!(
        snapshot.semantic.github.misbound_prs.iter().any(|pr| pr.number == 2010),
        "unknown-node trailer PR must be recorded as misbound"
    );
    Ok(())
}

#[test]
fn clean_surface_fixture_start_and_unbound_surface_reconcile() -> Result<()> {
    let snapshot = normalize_text(CLEAN_SURFACE_FIXTURE)?;
    // A pushed, clean, name-associated branch is an ownership decision, not a
    // silent START (the branch may be this node's unique work).
    let m01 = node(&snapshot, "M01")?;
    assert_eq!(m01.action, "RECONCILE");
    assert!(
        m01.action_reasons.iter().any(|reason| reason.starts_with("unbound_associated_surface:"))
    );
    assert!(m01.candidate_flags.contains(&"local_worktree".to_string()));
    // A ready node with no candidate and no surface STARTs (writer surface
    // available; ceilings are not quotas).
    assert_eq!(node(&snapshot, "M07A")?.action, "START");
    assert_eq!(node(&snapshot, "E00A")?.action, "START");
    Ok(())
}

#[test]
fn normalization_is_deterministic_and_observed_at_stays_outside_the_digest() -> Result<()> {
    let first = normalize_text(CORPUS_FIXTURE)?;
    let second = normalize_text(CORPUS_FIXTURE)?;
    let bytes_first = serde_json::to_vec_pretty(&first)?;
    let bytes_second = serde_json::to_vec_pretty(&second)?;
    assert_eq!(bytes_first, bytes_second, "two normalizations must be byte-identical");

    // Candidate order permutation moves no byte.
    let mut raw = raw_from_text(CORPUS_FIXTURE)?;
    raw.github.prs.reverse();
    let permuted = normalize_raw(&raw)?;
    let bytes_permuted = serde_json::to_vec_pretty(&permuted)?;
    assert_eq!(bytes_first, bytes_permuted, "candidate insertion order must not move bytes");

    // observed_at lives outside the semantic digest.
    let mut shifted = raw_from_text(CORPUS_FIXTURE)?;
    shifted.observed_at = "2030-01-01T00:00:00Z".to_string();
    let shifted_snapshot = normalize_raw(&shifted)?;
    assert_eq!(
        first.semantic_digest, shifted_snapshot.semantic_digest,
        "observed_at must not participate in the semantic digest"
    );
    assert_ne!(first.observed_at, shifted_snapshot.observed_at);
    Ok(())
}

#[test]
/// A snapshot written before #14237 must be rejected as an *older schema*,
/// never as a tampered one.
///
/// Those fields are `#[serde(default)]`, so a version-1 file still
/// deserializes — but its stored digest was computed over the old canonical
/// representation, so recomputing it after load necessarily disagrees. Left at
/// version 1 that disagreement came out of the tamper-detection path, which
/// accuses an honest operator of altering their snapshot and hides the real
/// remedy (re-run `refresh`). The version check must run first and own it.
#[test]
fn a_pre_change_snapshot_is_rejected_as_older_schema_not_as_tampering() -> Result<()> {
    let snapshot = normalize_text(CORPUS_FIXTURE)?;
    assert_eq!(snapshot.schema_version, LIVE_SCHEMA_VERSION);
    assert!(LIVE_SCHEMA_VERSION > 1, "the representation changed, so the version must too");

    // Stand in for a file written by the previous schema: same schema name,
    // the version it carried, and a digest that cannot match the new
    // representation.
    let mut legacy = snapshot.clone();
    legacy.schema_version = 1;
    let temp = std::env::temp_dir().join("module-train-live-legacy-v1.json");
    std::fs::write(&temp, serde_json::to_vec(&legacy)?)?;

    let error = load_snapshot(&temp)
        .err()
        .ok_or_else(|| color_eyre::eyre::eyre!("a superseded schema version must fail closed"))?;
    let text = error.to_string();
    assert!(text.contains("schema_version mismatch"), "got: {text}");
    assert!(
        !text.contains("digest drift"),
        "an older snapshot is not tampering, and must not be reported as it: {text}"
    );
    Ok(())
}

#[test]
fn snapshot_validation_detects_tampering() -> Result<()> {
    let snapshot = normalize_text(CORPUS_FIXTURE)?;
    let manifest = loaded()?;

    // Baseline: validation passes.
    assert!(validate_snapshot(&snapshot, &manifest).is_ok());

    // Digest drift: mutate one semantic fact.
    let mut tampered = snapshot.clone();
    tampered.semantic.nodes[0].c02_state = "ready".to_string();
    let bytes = serde_json::to_vec(&tampered)?;
    let temp = std::env::temp_dir().join("module-train-live-tamper-digest.json");
    std::fs::write(&temp, &bytes)?;
    let error = load_snapshot(&temp)
        .err()
        .ok_or_else(|| color_eyre::eyre::eyre!("digest drift must fail closed"))?;
    assert!(error.to_string().contains("digest drift"), "got: {error}");

    // Stored-action drift: rebuild a consistent digest around a wrong action.
    let mut drift = snapshot.clone();
    let index = drift
        .semantic
        .nodes
        .iter()
        .position(|node| node.node_id == "M07A")
        .ok_or_else(|| color_eyre::eyre::eyre!("M07A present"))?;
    drift.semantic.nodes[index].action = "MERGE_READY_RECOMMENDATION".to_string();
    let semantic_value = serde_json::to_value(&drift.semantic)?;
    drift.semantic_digest = canonical_digest(&semantic_value)?;
    let bytes = serde_json::to_vec(&drift)?;
    let temp = std::env::temp_dir().join("module-train-live-tamper-action.json");
    std::fs::write(&temp, &bytes)?;
    let reloaded = load_snapshot(&temp)?;
    let error = validate_snapshot(&reloaded, &manifest)
        .err()
        .ok_or_else(|| color_eyre::eyre::eyre!("stored action drift must fail validation"))?;
    assert!(error.to_string().contains("disagrees with re-derived"), "got: {error}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Instrument failures (falsifier 15).
// ---------------------------------------------------------------------------

fn raw_with_instrument(state: &str, instrument: &str) -> Result<RawObservation> {
    let mut raw = raw_from_text(CORPUS_FIXTURE)?;
    let record = serde_json::from_value::<InstrumentRecord>(serde_json::json!({
        "source": "test",
        "state": state,
        "detail": "forced by test"
    }))?;
    match instrument {
        "github_prs" => raw.instruments.github_prs = Some(record),
        "git_local" => raw.instruments.git_local = Some(record),
        "git_remote" => raw.instruments.git_remote = Some(record),
        other => color_eyre::eyre::bail!("unknown instrument {other}"),
    }
    Ok(raw)
}

#[test]
fn instrument_failures_are_not_proven_never_absence() -> Result<()> {
    for (state, instrument) in [
        ("failed", "github_prs"),
        ("rate_limited", "github_prs"),
        ("permission_denied", "github_prs"),
        ("truncated", "github_prs"),
        ("unavailable", "github_prs"),
        ("failed", "git_local"),
    ] {
        let raw = raw_with_instrument(state, instrument)?;
        let snapshot = normalize_raw(&raw).map_err(|error| {
            color_eyre::eyre::eyre!("{instrument}={state} must still normalize: {error}")
        })?;
        let m07a = node(&snapshot, "M07A")?;
        assert_eq!(
            m07a.action, "NOT_PROVEN",
            "{instrument}={state}: a failing instrument can never support START"
        );
        assert!(
            m07a.action_reasons.iter().any(|reason| reason.starts_with("instrument_")),
            "{instrument}={state}: reason codes must name the failed instrument"
        );
        assert!(m07a.candidate_flags.contains(&"instrument_failed".to_string()));
    }
    Ok(())
}

#[test]
fn git_remote_failure_degrades_only_remote_facts() -> Result<()> {
    let mut raw = raw_from_text(CLEAN_SURFACE_FIXTURE)?;
    let record = serde_json::from_value::<InstrumentRecord>(serde_json::json!({
        "source": "test", "state": "failed", "detail": "forced by test"
    }))?;
    raw.instruments.git_remote = Some(record);
    let snapshot = normalize_raw(&raw)?;
    // Classification stays possible (remote branches are diagnostics), with a
    // recorded limitation; remote surfaces are not projected as fact.
    let m07a = node(&snapshot, "M07A")?;
    assert_eq!(m07a.action, "START");
    assert!(
        m07a.limitations
            .contains(&"git_remote_observation_failed_remote_facts_not_proven".to_string()),
        "remote instrument failure must be a recorded limitation"
    );
    assert!(
        !m07a.surfaces.iter().any(|surface| surface.kind == "remote_branch"),
        "failed remote instrument must not project remote surfaces as facts"
    );
    Ok(())
}

#[test]
fn gone_upstream_counts_as_unpushed_unique_work() -> Result<()> {
    // A deleted upstream means local commits may exist nowhere else: the
    // branch is unique work and must gate START exactly like any other
    // unpushed surface (falsifier 9 family).
    let mut raw = raw_from_text(CLEAN_SURFACE_FIXTURE)?;
    raw.git_local.branches[0].upstream = Some("origin/tooling/8497-requests".to_string());
    raw.git_local.branches[0].ahead = Some(0);
    raw.git_local.branches[0].behind = None;
    raw.git_local.branches[0].upstream_gone = true;
    // Remove the pushed remote so only the gone-upstream fact remains.
    raw.git_remote.refs.retain(|reference| reference.name != "tooling/8497-requests");
    let snapshot = normalize_raw(&raw)?;
    let m01 = node(&snapshot, "M01")?;
    assert_eq!(
        m01.action, "RECONCILE",
        "a gone upstream must surface as unique work, never a clean pushed branch"
    );
    assert!(m01.candidate_flags.contains(&"dirty_or_unpushed_unique_work".to_string()));
    Ok(())
}

#[test]
fn tracking_parser_recognizes_gone_and_mixed_forms() {
    assert_eq!(parse_tracking("gone"), (None, None, true));
    assert_eq!(parse_tracking("ahead 2"), (Some(2), None, false));
    assert_eq!(parse_tracking("behind 3"), (None, Some(3), false));
    assert_eq!(parse_tracking("ahead 2, behind 1"), (Some(2), Some(1), false));
    assert_eq!(parse_tracking(""), (None, None, false));
}

#[test]
fn ancestry_probe_is_allowlist_gated_too() {
    // The ancestry path is the one non-string adapter; it must reject any
    // argument shape outside the read-only allowlist exactly like the choke
    // point does (structural read-only law covers every spawn path). The
    // hostile "oid" never reaches git: exit-1/0 would mean it spawned.
    match run_git_ancestry(Path::new("."), "abc; rm -rf /") {
        Ancestry::ProbeFailed(reason) => {
            assert!(
                reason.contains("read-only") || reason.contains("non-hex"),
                "unexpected refusal text: {reason}"
            )
        }
        reached => assert!(
            false,
            "hostile oid must never reach git; got {reached:?} for a probe that must be gated"
        ),
    }
}

// ---------------------------------------------------------------------------
// Bot-review repairs (PR #12217 findings): repo-bound gh queries, fail-closed
// detail reads, partial-trailer retention, manifest-digest validation,
// cancelled checks without verdict.
// ---------------------------------------------------------------------------

#[test]
fn gh_queries_are_bound_to_the_checkout_repository() {
    // GH_REPO must never redirect observation to a foreign repository: every
    // gh argument list carries the origin-derived --repo selector.
    for args in [
        gh_list_args("open", OPEN_PR_LIMIT, "EffortlessMetrics/perl-lsp-swarm"),
        gh_list_args("merged", MERGED_PR_WINDOW, "fork/other"),
        gh_view_args(3001, "EffortlessMetrics/perl-lsp-swarm"),
    ] {
        let repo_index = args
            .iter()
            .position(|arg| arg == "--repo")
            .unwrap_or_else(|| panic!("--repo missing from {args:?}"));
        assert!(
            args.get(repo_index + 1).is_some_and(|value| !value.is_empty()),
            "--repo selector must carry a value in {args:?}"
        );
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        assert!(args_read_only("gh", &borrowed), "repo-bound gh shapes stay read-only: {args:?}");
    }

    // The GraphQL read carries the same origin-derived selector, split into
    // the document's typed owner/name variables rather than a --repo flag. It
    // must be just as repo-bound: an unqualified query would observe whatever
    // repository the ambient environment names.
    let graphql = gh_graphql_review_args(3001, "EffortlessMetrics", "perl-lsp-swarm");
    assert!(
        graphql.iter().any(|arg| arg == "owner=EffortlessMetrics"),
        "graphql query must carry the origin-derived owner: {graphql:?}"
    );
    assert!(
        graphql.iter().any(|arg| arg == "name=perl-lsp-swarm"),
        "graphql query must carry the origin-derived name: {graphql:?}"
    );
    assert!(
        graphql.iter().any(|arg| arg == "pr=3001"),
        "graphql query must carry the exact PR number: {graphql:?}"
    );
    let borrowed: Vec<&str> = graphql.iter().map(String::as_str).collect();
    assert!(args_read_only("gh", &borrowed), "the graphql shape stays read-only: {graphql:?}");
}

#[test]
fn incomplete_identity_block_still_names_its_node() -> Result<()> {
    // The corpus PR #2012 carries only `Module node: #10571`: the block cannot
    // bind, but M04D must surface the claim (RECONCILE misbound) instead of
    // silently STARTing duplicate work.
    let snapshot = normalize_text(CORPUS_FIXTURE)?;
    let m04d = node(&snapshot, "M04D")?;
    assert_eq!(m04d.action, "RECONCILE", "partial-block claim must gate START");
    assert!(m04d.candidate_flags.contains(&"misbound_candidate".to_string()));
    assert!(
        m04d.action_reasons.contains(&"misbound_candidate_pr:#2012".to_string()),
        "got reasons {:?}",
        m04d.action_reasons
    );
    // The PR itself is recorded with its node association retained.
    assert!(
        snapshot
            .semantic
            .github
            .misbound_prs
            .iter()
            .any(|pr| pr.number == 2012 && pr.node_id.as_deref() == Some("M04D")),
        "partial-block PR must stay attached to its named node"
    );
    Ok(())
}

#[test]
fn cancelled_checks_carry_no_verdict() {
    let raw_pr = RawPr {
        number: 4001,
        state: "OPEN".to_string(),
        head_oid: "dddddddddddddddddddddddddddddddddddddddd".to_string(),
        checks: Some(vec![RawCheck {
            name: "superseded run".to_string(),
            status: "COMPLETED".to_string(),
            conclusion: "CANCELLED".to_string(),
        }]),
        ..raw_pr_from_list(&serde_json::json!({
            "number": 4001,
            "state": "OPEN",
            "headRefOid": "dddddddddddddddddddddddddddddddddddddddd"
        }))
    };
    let facts = checks_facts(&raw_pr);
    assert_eq!(facts.failed, 0, "a cancelled run is not a failure verdict");
    assert_eq!(facts.cancelled, 1);

    // And the classifier records it as a limitation, never REPAIR.
    let mut facts = facts_base();
    facts.open_bound = vec![CandidateView { checks_cancelled: true, ..open_candidate() }];
    let classified = classify(&facts);
    assert_ne!(classified.action, Action::Repair);
    assert!(
        classified.limitations.contains(&"checks_cancelled_no_verdict_recorded".to_string()),
        "got limitations {:?}",
        classified.limitations
    );
}

#[test]
fn validation_binds_the_snapshot_to_the_current_manifest() -> Result<()> {
    let snapshot = normalize_text(CORPUS_FIXTURE)?;
    let manifest = loaded()?;

    // Foreign manifest digest: even a re-digested (internally consistent)
    // snapshot from a different train revision must fail closed.
    let mut foreign = snapshot.clone();
    foreign.semantic.train.manifest_digest = "0".repeat(64);
    let value = serde_json::to_value(&foreign.semantic)?;
    foreign.semantic_digest = canonical_digest(&value)?;
    let error = validate_snapshot(&foreign, &manifest)
        .err()
        .ok_or_else(|| color_eyre::eyre::eyre!("foreign manifest digest must fail"))?;
    assert!(error.to_string().contains("does not match the pinned"), "got: {error}");

    // Node-set disagreement with the manifest also fails closed.
    let mut pruned = snapshot.clone();
    pruned.semantic.nodes.truncate(5);
    let value = serde_json::to_value(&pruned.semantic)?;
    pruned.semantic_digest = canonical_digest(&value)?;
    let error = validate_snapshot(&pruned, &manifest)
        .err()
        .ok_or_else(|| color_eyre::eyre::eyre!("pruned node set must fail"))?;
    assert!(error.to_string().contains("node set disagrees"), "got: {error}");
    Ok(())
}

#[test]
fn truncation_degrades_precisely_not_globally() -> Result<()> {
    // Open-window truncation: absence of a viable candidate is not provable,
    // so every node gates to NOT_PROVEN (falsifier 15, list side).
    let mut raw = raw_from_text(CORPUS_FIXTURE)?;
    raw.github.open_truncated = true;
    let snapshot = normalize_raw(&raw)?;
    let m01 = node(&snapshot, "M01")?;
    assert_eq!(m01.action, "NOT_PROVEN");
    assert!(m01.action_reasons.contains(&"instrument_github_failed".to_string()));

    // Merged-window truncation (this repository's merge velocity makes any
    // bounded merged window truncated): only merged facts degrade; viable
    // open candidates still classify, with a recorded limitation.
    let mut raw = raw_from_text(CORPUS_FIXTURE)?;
    raw.github.merged_truncated = true;
    raw.instruments.github_prs = Some(InstrumentRecord {
        source: "test".to_string(),
        state: InstrumentState::Ok,
        detail: "merged PR window hit its limit (100); ".to_string(),
    });
    let snapshot = normalize_raw(&raw)?;
    let m01 = node(&snapshot, "M01")?;
    assert_eq!(m01.action, "REVIEW", "merged-window truncation must not gate viability");
    assert!(
        m01.limitations.contains(&"merged_window_truncated_merged_facts_not_proven".to_string()),
        "merged-window truncation must be a recorded limitation"
    );
    Ok(())
}

#[test]
fn gh_failure_classification_vocabulary() {
    assert_eq!(
        InstrumentState::from_failure_text("gh: API rate limit exceeded for installation"),
        InstrumentState::RateLimited
    );
    assert_eq!(
        InstrumentState::from_failure_text("gh: HTTP 403 Forbidden (resource owned by other)"),
        InstrumentState::PermissionDenied
    );
    assert_eq!(
        InstrumentState::from_failure_text("gh: HTTP 404 Not Found"),
        InstrumentState::Unavailable
    );
    assert_eq!(InstrumentState::from_failure_text("gh: connection reset"), InstrumentState::Failed);
}

// ---------------------------------------------------------------------------
// Snapshot-to-renderer path (check/next/explain offline over a written file).
// ---------------------------------------------------------------------------

#[test]
fn written_snapshot_round_trips_through_check_next_explain() -> Result<()> {
    let snapshot = normalize_text(CORPUS_FIXTURE)?;
    let temp = std::env::temp_dir().join("module-train-live-roundtrip.json");
    let bytes = serde_json::to_vec_pretty(&snapshot)?;
    std::fs::write(&temp, bytes)?;
    let reloaded = load_snapshot(&temp)?;
    let report = validate_snapshot(&reloaded, &loaded()?)?;
    assert_eq!(report.len(), reloaded.semantic.nodes.len());

    let next = render_next(&reloaded);
    assert!(next.contains("RECONCILE"));
    assert!(next.contains("M07A"));
    assert!(next.contains("at most one action per writer/conflict surface"));
    // START remains reachable on a clean frontier (clean-surface fixture).
    let clean = normalize_text(CLEAN_SURFACE_FIXTURE)?;
    let clean_next = render_next(&clean);
    assert!(
        clean_next.contains("START (3)"),
        "clean frontier must START its ready leaves: {clean_next}"
    );

    let explain = render_explain(&reloaded, &loaded()?, "C03")?;
    assert!(explain.contains("module-train live explain C03"));
    assert!(explain.contains("action: BLOCKED"));
    assert!(explain.contains("closeout route"));
    assert!(render_explain(&reloaded, &loaded()?, "NOPE").is_err());
    Ok(())
}

#[test]
fn corpus_bodies_are_never_stored() -> Result<()> {
    let snapshot = normalize_text(CORPUS_FIXTURE)?;
    let bytes = serde_json::to_vec_pretty(&snapshot)?;
    let text = String::from_utf8(bytes)?;
    assert!(!text.contains("Implementation of validated requests"), "bodies must be dropped");
    assert!(!text.contains("\"body\""), "no body field may exist in the snapshot");
    Ok(())
}
