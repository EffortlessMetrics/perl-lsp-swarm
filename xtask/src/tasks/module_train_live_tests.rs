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

fn probe() -> Result<(RepoTreeSource, Option<String>)> {
    Ok((RepoTreeSource::from_project_root()?, Some(current_head()?)))
}

/// HEAD of the executing checkout, so a test can build a coherent probe.
fn current_head() -> Result<String> {
    Ok(tree_binding("HEAD")?.tree_head)
}

fn normalize_raw(raw: &RawObservation) -> Result<LiveSnapshot> {
    let (source, head) = probe()?;
    normalize(raw, &loaded()?, &TreeProbe { source: &source, head, dirty: false })
}

fn normalize_text(text: &str) -> Result<LiveSnapshot> {
    normalize_raw(&raw_from_text(text)?)
}

fn normalize_clean_surface() -> Result<LiveSnapshot> {
    let mut raw = raw_from_text(CLEAN_SURFACE_FIXTURE)?;
    raw.git_local.head = Some(current_head()?);
    normalize_raw(&raw)
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
// Probed-tree identity (#11626 review finding on #15094).
// ---------------------------------------------------------------------------

/// A stored fixture records a synthetic head, so its observation never
/// describes the executing checkout. The join is still emitted, but every node
/// must say the implementation states came from a different tree.
#[test]
fn a_fixture_observation_marks_its_states_as_probed_from_another_tree() -> Result<()> {
    let snapshot = normalize_text(CORPUS_FIXTURE)?;
    for node in &snapshot.semantic.nodes {
        assert!(
            node.limitations.iter().any(|l| l == PROBED_FROM_A_DIFFERENT_TREE),
            "node {} must record the probed-tree mismatch: {:?}",
            node.node_id,
            node.limitations
        );
    }
    Ok(())
}

/// The opposite direction: when the observation's head IS the probed tree, no
/// node carries the limitation. Without this the assertion above would pass on
/// an implementation that always sets it.
#[test]
fn a_coherent_observation_carries_no_probed_tree_limitation() -> Result<()> {
    let mut raw = raw_from_text(CORPUS_FIXTURE)?;
    raw.git_local.head = Some(current_head()?);
    let snapshot = normalize_raw(&raw)?;
    for node in &snapshot.semantic.nodes {
        assert!(
            !node.limitations.iter().any(|l| l == PROBED_FROM_A_DIFFERENT_TREE),
            "node {} must not claim a mismatch when heads agree: {:?}",
            node.node_id,
            node.limitations
        );
    }
    Ok(())
}

/// A matching HEAD is insufficient when the probe reads a mutable working
/// tree: dirty content cannot be shown equivalent to the commit-only record.
#[test]
fn a_dirty_probe_fails_closed_even_when_heads_agree() -> Result<()> {
    let mut raw = raw_from_text(CORPUS_FIXTURE)?;
    raw.git_local.head = Some(current_head()?);
    let (source, head) = probe()?;
    let snapshot = normalize(&raw, &loaded()?, &TreeProbe { source: &source, head, dirty: true })?;
    if !snapshot
        .semantic
        .nodes
        .iter()
        .all(|node| node.limitations.iter().any(|l| l == PROBED_FROM_A_DIFFERENT_TREE))
    {
        color_eyre::eyre::bail!("dirty probe must fail closed even when HEADs agree");
    }
    if node(&snapshot, "M07A")?.action != "NOT_PROVEN" {
        color_eyre::eyre::bail!("dirty probe must gate tree-dependent START actions");
    }
    Ok(())
}

#[test]
fn a_dirty_manifest_fails_closed_even_without_dirty_path_rows() -> Result<()> {
    let mut raw = raw_from_text(CORPUS_FIXTURE)?;
    raw.git_local.head = Some(current_head()?);
    raw.git_local.manifest_dirty = true;
    let (source, head) = probe()?;
    let snapshot = normalize(&raw, &loaded()?, &TreeProbe { source: &source, head, dirty: false })?;
    if !snapshot
        .semantic
        .nodes
        .iter()
        .all(|node| node.limitations.iter().any(|l| l == PROBED_FROM_A_DIFFERENT_TREE))
    {
        color_eyre::eyre::bail!("manifest-dirty observation must fail closed");
    }
    Ok(())
}

/// An unestablishable probed head fails closed rather than silently claiming
/// the observation and the tree agree.
#[test]
fn an_unknown_probed_head_fails_closed() -> Result<()> {
    let mut raw = raw_from_text(CORPUS_FIXTURE)?;
    raw.git_local.head = Some(current_head()?);
    let (source, _) = probe()?;
    let snapshot =
        normalize(&raw, &loaded()?, &TreeProbe { source: &source, head: None, dirty: false })?;
    assert!(
        snapshot
            .semantic
            .nodes
            .iter()
            .all(|node| node.limitations.iter().any(|l| l == PROBED_FROM_A_DIFFERENT_TREE)),
        "an unknown probed head must not read as agreement"
    );
    if node(&snapshot, "M07A")?.action != "NOT_PROVEN" {
        color_eyre::eyre::bail!("an unknown probed head must gate tree-dependent START actions");
    }
    Ok(())
}

#[test]
fn mismatched_tree_gates_start_but_keeps_candidate_action() -> Result<()> {
    let mut mismatched_raw = raw_from_text(CLEAN_SURFACE_FIXTURE)?;
    mismatched_raw.git_local.head = Some("f".repeat(40));
    let snapshot = normalize_raw(&mismatched_raw)?;
    let start_leaf = node(&snapshot, "M07A")?;
    if start_leaf.action != "NOT_PROVEN"
        || !start_leaf
            .limitations
            .iter()
            .any(|limitation| limitation == PROBED_FROM_A_DIFFERENT_TREE)
        || start_leaf.c02_state != "not_proven"
        || start_leaf.c02_reasons != vec![PROBED_FROM_A_DIFFERENT_TREE.to_string()]
    {
        color_eyre::eyre::bail!(
            "a mismatched ready leaf must not START: action={} limitations={:?}",
            start_leaf.action,
            start_leaf.limitations
        );
    }
    let explain = render_explain(&snapshot, &loaded()?, "M07A")?;
    let expected_state = format!("c02_state: not_proven reasons={PROBED_FROM_A_DIFFERENT_TREE}");
    if !explain.contains(&expected_state) || explain.contains("c02_state: ready") {
        color_eyre::eyre::bail!(
            "mismatched-tree explain must expose the effective NOT_PROVEN state: {explain}"
        );
    }
    let candidate_snapshot = normalize_text(CORPUS_FIXTURE)?;
    let candidate = node(&candidate_snapshot, "M01")?;
    if candidate.action != "REVIEW" {
        color_eyre::eyre::bail!(
            "a candidate-only review action should remain actionable: {}",
            candidate.action
        );
    }
    Ok(())
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
            "gh api",
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
    // No merge-base shape: ancestry resolves through the shared
    // `xtask::git_ancestry` authority now, not through an observation spawn
    // (#14557), so the read-only allowlist no longer carries it.
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
        ("C01", "NOT_PROVEN", "c02_state_not_actionable:not_proven"),
        ("C02", "WAIT", "landed_current_tree_no_writer_action"),
        // C03's implementation is on the tree and its semantic probe (#11626)
        // now sees it, so it is landed rather than statically blocked on C02.
        ("C03", "NOT_PROVEN", "c02_state_not_actionable:not_proven"),
        ("CTRL", "STOP", "controller_selected_as_implementation"),
        ("E00A", "REPAIR", "review_changes_requested"),
        ("E00C", "RECONCILE", "multiple_bound_candidates_need_bounded_ownership_decision"),
        ("M01", "REVIEW", "review_pending"),
        ("M07A", "NOT_PROVEN", "c02_state_not_actionable:not_proven"),
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
    // Falsifier 7: stray issue closure/labels changed nothing — M01 and C03
    // are still classified from the train + candidate facts alone. C03's
    // action follows its #11626 current-tree probe, never its issue state.
    let m01 = node(&snapshot, "M01")?;
    assert_eq!(m01.action, "REVIEW");
    assert_eq!(
        node(&snapshot, "C03")?.action,
        "NOT_PROVEN",
        "C03 cannot use a mismatched current-tree probe"
    );
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
    let snapshot = normalize_clean_surface()?;
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
    drift.semantic.nodes[index].action = "START".to_string();
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

#[test]
fn cross_tree_validation_rejects_false_stored_state() -> Result<()> {
    // Devin review, PR #15094: the validator substitutes honest not_proven
    // values for cross-tree nodes during re-derivation; a snapshot rebuilt
    // with a self-consistent digest around a false stored state must not be
    // laundered through that substitution.
    let snapshot = normalize_text(CORPUS_FIXTURE)?;
    let manifest = loaded()?;
    let marker = "c02_implementation_probed_from_a_different_tree";

    let mut forged = snapshot.clone();
    // Mark every node (the producer's marker is snapshot-wide) and forge the
    // false stored state on the first; the stored-state check must catch it.
    for node in &mut forged.semantic.nodes {
        node.limitations.push(marker.to_string());
        node.limitations.sort();
    }
    let node = &mut forged.semantic.nodes[0];
    node.c02_state = "ready".to_string();
    node.c02_reasons = Vec::new();
    let semantic_value = serde_json::to_value(&forged.semantic)?;
    forged.semantic_digest = canonical_digest(&semantic_value)?;

    let error = validate_snapshot(&forged, &manifest)
        .err()
        .ok_or_else(|| color_eyre::eyre::eyre!("false cross-tree state must fail validation"))?;
    assert!(
        error.to_string().contains("must record not_proven"),
        "the failure must name the honest-record invariant, got: {error}"
    );
    Ok(())
}

#[test]
fn cross_tree_marker_must_be_snapshot_wide() -> Result<()> {
    // Devin review, PR #15094: the producer computes the cross-tree condition
    // once per snapshot, so a marker on only some nodes is itself evidence of
    // a stale or tampered record — and an unmarked node would skip the
    // stored-state check.
    let snapshot = normalize_text(CORPUS_FIXTURE)?;
    let manifest = loaded()?;
    let marker = "c02_implementation_probed_from_a_different_tree";

    // The corpus observation is cross-tree, so every node already carries the
    // marker and the honest not_proven record. Removing the marker from one
    // node creates the mixed set a stale or tampered producer would emit.
    let mut forged = snapshot.clone();
    let node = &mut forged.semantic.nodes[1];
    node.limitations.retain(|limitation| limitation != marker);
    let semantic_value = serde_json::to_value(&forged.semantic)?;
    forged.semantic_digest = canonical_digest(&semantic_value)?;

    let error = validate_snapshot(&forged, &manifest)
        .err()
        .ok_or_else(|| color_eyre::eyre::eyre!("a mixed marker set must fail validation"))?;
    assert!(
        error.to_string().contains("snapshot-wide"),
        "the failure must name the uniformity invariant, got: {error}"
    );
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
    raw.git_local.head = Some(current_head()?);
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
    raw.git_local.head = Some(current_head()?);
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
fn ancestry_probe_rejects_hostile_oid_before_git() {
    // The ancestry path resolves through the shared authority, but the oid
    // shape gate stays in this module: the hostile "oid" never reaches any
    // probe. A `ProbeFailed` carrying the refusal is the only acceptable
    // outcome.
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
    let clean = normalize_clean_surface()?;
    let clean_next = render_next(&clean);
    assert!(
        clean_next.contains("START (2)"),
        "clean frontier must START its ready leaves: {clean_next}"
    );

    let explain = render_explain(&reloaded, &loaded()?, "C03")?;
    assert!(explain.contains("module-train live explain C03"));
    assert!(explain.contains("action: NOT_PROVEN"));
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
