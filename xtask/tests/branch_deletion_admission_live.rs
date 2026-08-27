//! Live-collection falsifiers for branch-deletion admission (#12885).
//!
//! The adapter is driven through a fake command surface, so every case is
//! hermetic: no network, no `gh`, no real repository, and nothing mutated.
//! What is under test is that live collection is *fail-closed* — an
//! unreadable, unparseable, or truncated read must not become a permissive
//! default.

use std::collections::HashMap;

use std::cell::RefCell;

use xtask::branch_deletion_admission::{
    DeletionAdmission, DeletionExecutor, ReadOnlyCommands, branch_deletion_command,
    collect_request, evaluate, execute_admitted_deletion, repository_from_remote_url,
    verify_remote_identity,
};

const BRANCH: &str = "agent/vim-activation-root-7762";
const HEAD_SHA: &str = "1111111111111111111111111111111111111111";

/// Canned command surface. Any command not explicitly stubbed fails, so a
/// test cannot accidentally pass by reading something it never set up.
#[derive(Default)]
struct FakeCommands {
    responses: HashMap<String, Result<String, String>>,
}

impl FakeCommands {
    fn key(program: &str, args: &[&str]) -> String {
        format!("{program} {}", args.join(" "))
    }

    /// Stub by the leading tokens of a command, so long `--json` field lists
    /// do not have to be repeated in every test.
    ///
    /// A later registration supersedes any narrower stub it covers, so a test
    /// can override `healthy()`'s `git ls-remote origin` with a broader
    /// `git ls-remote` failure. Without this, longest-prefix resolution would
    /// silently keep the healthy stub and the test would pass for the wrong
    /// reason.
    fn stub(mut self, prefix: &str, response: Result<String, String>) -> Self {
        self.responses.retain(|existing, _| !existing.starts_with(prefix));
        self.responses.insert(prefix.to_string(), response);
        self
    }

    fn on(self, prefix: &str, output: &str) -> Self {
        self.stub(prefix, Ok(output.to_string()))
    }

    fn failing(self, prefix: &str, error: &str) -> Self {
        self.stub(prefix, Err(error.to_string()))
    }
}

impl ReadOnlyCommands for FakeCommands {
    fn capture(&self, program: &str, args: &[&str]) -> color_eyre::eyre::Result<String> {
        // Reject anything that could mutate: the adapter must stay read-only.
        let mutating = ["push", "delete", "commit", "merge", "close", "edit", "create"];
        for argument in args {
            assert!(
                !mutating.contains(argument),
                "live collection issued a mutating command: {program} {args:?}",
            );
        }

        let full = Self::key(program, args);
        let matched = self
            .responses
            .iter()
            .filter(|(prefix, _)| full.starts_with(prefix.as_str()))
            .max_by_key(|(prefix, _)| prefix.len());
        match matched {
            Some((_, Ok(output))) => Ok(output.clone()),
            Some((_, Err(error))) => Err(color_eyre::eyre::eyre!("{error}")),
            None => Err(color_eyre::eyre::eyre!("unstubbed command: {full}")),
        }
    }
}

fn merged_parent_json() -> String {
    format!(
        r#"{{"number":7799,"state":"MERGED","merged":true,"headRefName":"{BRANCH}","headRefOid":"{HEAD_SHA}"}}"#
    )
}

/// Every read succeeds and reports an unencumbered subject.
fn healthy() -> FakeCommands {
    FakeCommands::default()
        .on(
            "git remote get-url origin",
            "https://github.com/EffortlessMetrics/perl-lsp-swarm.git\n",
        )
        .on("gh pr view 7799", &merged_parent_json())
        .on("gh pr list", "[]")
        .on("git ls-remote origin", &format!("{HEAD_SHA}\trefs/heads/{BRANCH}\n"))
        .on("git worktree list", "worktree /repo\nHEAD abc\nbranch refs/heads/main\n")
}

/// Positive control. Without it every fail-closed case below could pass for
/// the trivial reason that live collection never admits anything.
#[test]
fn a_fully_read_unencumbered_subject_is_admitted() -> Result<(), Box<dyn std::error::Error>> {
    let request = collect_request(&healthy(), 7799, "origin")?;
    let outcome = evaluate(&request);
    assert_eq!(outcome.admission, DeletionAdmission::SafeToDelete, "{}", outcome.detail);
    assert_eq!(outcome.repository, "EffortlessMetrics/perl-lsp-swarm");
    assert_eq!(outcome.admitted_sha.as_deref(), Some(HEAD_SHA));
    assert!(branch_deletion_command(&outcome).is_some());
    Ok(())
}

/// Repository identity is derived from the remote, not supplied by the
/// caller, so a live plan cannot be aimed at a repository the child check
/// never covered.
#[test]
fn repository_identity_comes_from_the_remote() -> Result<(), Box<dyn std::error::Error>> {
    for (url, expected) in [
        (
            "https://github.com/EffortlessMetrics/perl-lsp-swarm.git",
            "EffortlessMetrics/perl-lsp-swarm",
        ),
        ("https://github.com/EffortlessMetrics/perl-lsp-swarm", "EffortlessMetrics/perl-lsp-swarm"),
        ("git@github.com:EffortlessMetrics/perl-lsp-swarm.git", "EffortlessMetrics/perl-lsp-swarm"),
        ("ssh://git@github.example.com/Some/Fork.git", "Some/Fork"),
    ] {
        let parsed = repository_from_remote_url(url).ok_or_else(|| format!("{url} must parse"))?;
        assert_eq!(parsed.render(), expected, "for {url}");
    }

    // Unparseable forms must not be guessed at.
    for url in ["", "not-a-url", "https://github.com/onlyowner"] {
        assert!(repository_from_remote_url(url).is_none(), "{url:?} must not parse");
    }
    Ok(())
}

/// `remote_verification_command` only names the check; the live path must
/// actually run it and refuse a mismatch.
#[test]
fn remote_identity_is_verified_not_merely_named() {
    let commands = healthy();
    assert!(
        verify_remote_identity(&commands, "origin", "EffortlessMetrics/perl-lsp-swarm").is_ok(),
        "the matching repository must verify",
    );
    assert!(
        verify_remote_identity(&commands, "origin", "SomeoneElse/perl-lsp-swarm").is_err(),
        "a different repository must be refused, not accepted",
    );

    let unreadable = FakeCommands::default().failing("git remote get-url", "no such remote");
    assert!(
        verify_remote_identity(&unreadable, "origin", "EffortlessMetrics/perl-lsp-swarm").is_err(),
        "an unreadable remote must be an error, never a pass",
    );
}

/// An unreadable or unparseable child listing must retain. "The listing
/// failed" is not "there are no children".
#[test]
fn an_unreadable_child_listing_retains() -> Result<(), Box<dyn std::error::Error>> {
    let unreachable = healthy().failing("gh pr list", "gh: could not reach api.github.com");
    let outcome = evaluate(&collect_request(&unreachable, 7799, "origin")?);
    assert_eq!(outcome.admission, DeletionAdmission::RetainGraphNotProven, "{}", outcome.detail);
    assert_eq!(branch_deletion_command(&outcome), None);

    let garbled = healthy().on("gh pr list", "{not json");
    let outcome = evaluate(&collect_request(&garbled, 7799, "origin")?);
    assert_eq!(outcome.admission, DeletionAdmission::RetainGraphNotProven, "{}", outcome.detail);
    Ok(())
}

/// A listing returned at the page limit may be truncated, so it must report
/// as such rather than be read as the complete graph.
#[test]
fn a_page_limit_listing_is_reported_as_truncated() -> Result<(), Box<dyn std::error::Error>> {
    let rows: Vec<String> = (1..=100)
        .map(|n| {
            format!(
                r#"{{"number":{n},"state":"OPEN","isDraft":false,"headRefName":"h{n}","baseRefName":"other","mergeable":"MERGEABLE"}}"#
            )
        })
        .collect();
    let full_page = healthy().on("gh pr list", &format!("[{}]", rows.join(",")));

    // Every row targets a different base, so only truncation can retain here.
    let outcome = evaluate(&collect_request(&full_page, 7799, "origin")?);
    assert_eq!(
        outcome.admission,
        DeletionAdmission::RetainGraphNotProven,
        "a full page must not be read as a complete graph: {}",
        outcome.detail,
    );
    Ok(())
}

/// A live open child on the parent's branch retains, carrying the identity a
/// reconciler needs.
#[test]
fn a_live_open_child_retains() -> Result<(), Box<dyn std::error::Error>> {
    let with_child = healthy().on(
        "gh pr list",
        &format!(
            r#"[{{"number":7810,"state":"OPEN","isDraft":true,"headRefName":"agent/child","baseRefName":"{BRANCH}","mergeable":"CONFLICTING"}}]"#
        ),
    );
    let outcome = evaluate(&collect_request(&with_child, 7799, "origin")?);
    assert_eq!(outcome.admission, DeletionAdmission::RetainOpenChildren, "{}", outcome.detail);
    assert_eq!(outcome.retained_children.len(), 1);
    assert_eq!(outcome.retained_children[0].number, 7810);
    assert!(outcome.retained_children[0].draft);
    assert_eq!(branch_deletion_command(&outcome), None);
    Ok(())
}

/// An unreadable branch tip is movement, not agreement; and a tip that
/// disagrees with the reviewed subject is refused.
#[test]
fn an_unreadable_or_moved_tip_retains() -> Result<(), Box<dyn std::error::Error>> {
    let unreadable = healthy().failing("git ls-remote", "connection reset");
    let outcome = evaluate(&collect_request(&unreadable, 7799, "origin")?);
    assert_eq!(outcome.admission, DeletionAdmission::RetainBranchMoved, "{}", outcome.detail);

    let advanced = healthy().on(
        "git ls-remote origin",
        &format!("2222222222222222222222222222222222222222\trefs/heads/{BRANCH}\n"),
    );
    let outcome = evaluate(&collect_request(&advanced, 7799, "origin")?);
    assert_eq!(outcome.admission, DeletionAdmission::RetainBranchMoved, "{}", outcome.detail);
    Ok(())
}

/// A local worktree holding the branch blocks deletion, and an unreadable
/// worktree list is `NOT_PROVEN` rather than `Clear` — #3957 owns this signal
/// and absence of evidence is not evidence of absence.
#[test]
fn local_worktree_ownership_blocks_and_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
    let owned = healthy().on(
        "git worktree list",
        &format!("worktree /repo/wt-7762\nHEAD {HEAD_SHA}\nbranch refs/heads/{BRANCH}\n"),
    );
    let outcome = evaluate(&collect_request(&owned, 7799, "origin")?);
    assert_eq!(outcome.admission, DeletionAdmission::RetainGraphNotProven, "{}", outcome.detail);
    assert!(
        outcome.detail.contains("wt-7762"),
        "the detail must name the worktree: {}",
        outcome.detail
    );

    let unreadable = healthy().failing("git worktree list", "not a git repository");
    let outcome = evaluate(&collect_request(&unreadable, 7799, "origin")?);
    assert_eq!(outcome.admission, DeletionAdmission::RetainGraphNotProven, "{}", outcome.detail);
    Ok(())
}

/// A parent that is not merged retains, whatever else is true.
#[test]
fn a_non_terminal_parent_retains() -> Result<(), Box<dyn std::error::Error>> {
    for (state, merged) in [("OPEN", false), ("CLOSED", false)] {
        let commands = healthy().on(
            "gh pr view 7799",
            &format!(
                r#"{{"number":7799,"state":"{state}","merged":{merged},"headRefName":"{BRANCH}","headRefOid":"{HEAD_SHA}"}}"#
            ),
        );
        let outcome = evaluate(&collect_request(&commands, 7799, "origin")?);
        assert_eq!(
            outcome.admission,
            DeletionAdmission::RetainParentNotTerminal,
            "state {state} must retain: {}",
            outcome.detail,
        );
    }
    Ok(())
}

/// An unreadable remote or parent is a hard error: collection cannot invent a
/// subject, so there is nothing to evaluate.
#[test]
fn an_unreadable_subject_is_an_error_not_a_default() {
    let no_remote = FakeCommands::default().failing("git remote get-url", "no such remote");
    assert!(collect_request(&no_remote, 7799, "origin").is_err());

    let no_parent = healthy().failing("gh pr view", "gh: pull request not found");
    assert!(collect_request(&no_parent, 7799, "origin").is_err());

    let garbled_parent = healthy().on("gh pr view 7799", "{not json");
    assert!(collect_request(&garbled_parent, 7799, "origin").is_err());
}

/// Records what the deletion path would run, without running anything.
#[derive(Default)]
struct RecordingDeleter {
    invocations: RefCell<Vec<Vec<String>>>,
}

impl DeletionExecutor for RecordingDeleter {
    fn execute(&self, argv: &[String]) -> color_eyre::eyre::Result<()> {
        self.invocations.borrow_mut().push(argv.to_vec());
        Ok(())
    }
}

/// The deletion path must refuse every retaining outcome, and must not reach
/// the executor at all. This is the property the August 15 incident violated.
#[test]
fn the_deletion_path_refuses_every_retaining_outcome() -> Result<(), Box<dyn std::error::Error>> {
    let retaining = [
        healthy().on(
            "gh pr list",
            &format!(
                r#"[{{"number":7810,"state":"OPEN","isDraft":false,"headRefName":"c","baseRefName":"{BRANCH}","mergeable":"MERGEABLE"}}]"#
            ),
        ),
        healthy().failing("gh pr list", "api unreachable"),
        healthy().failing("git ls-remote", "connection reset"),
        healthy().on(
            "gh pr view 7799",
            &format!(
                r#"{{"number":7799,"state":"OPEN","merged":false,"headRefName":"{BRANCH}","headRefOid":"{HEAD_SHA}"}}"#
            ),
        ),
    ];

    for commands in retaining {
        let outcome = evaluate(&collect_request(&commands, 7799, "origin")?);
        let deleter = RecordingDeleter::default();
        let result = execute_admitted_deletion(&commands, &deleter, &outcome);
        assert!(result.is_err(), "{:?} must refuse deletion", outcome.admission);
        assert!(
            deleter.invocations.borrow().is_empty(),
            "a retaining outcome must never reach the executor: {:?}",
            deleter.invocations.borrow(),
        );
    }
    Ok(())
}

/// An admitted outcome runs exactly the leased argv — as a vector, never a
/// shell string. A branch name carrying shell metacharacters must arrive as
/// one argument rather than becoming a command.
#[test]
fn an_admitted_deletion_runs_the_leased_argv_without_a_shell()
-> Result<(), Box<dyn std::error::Error>> {
    let outcome = evaluate(&collect_request(&healthy(), 7799, "origin")?);
    let deleter = RecordingDeleter::default();
    execute_admitted_deletion(&healthy(), &deleter, &outcome)?;

    let invocations = deleter.invocations.borrow();
    assert_eq!(invocations.len(), 1, "exactly one deletion");
    assert_eq!(
        invocations[0],
        branch_deletion_command(&outcome).unwrap_or_default(),
        "the executed argv must be the leased command verbatim",
    );
    // argv, not a shell line: the vector has discrete arguments and no
    // element is a concatenated command string.
    assert!(invocations[0].len() >= 5, "{:?}", invocations[0]);
    assert!(
        !invocations[0].iter().any(|argument| argument.contains(' ')),
        "no argument may be a packed shell string: {:?}",
        invocations[0],
    );
    Ok(())
}

/// A branch whose name contains shell metacharacters must travel as a single
/// argument. Under the previous `eval`-based design this was a command
/// injection; here it is inert data.
#[test]
fn a_branch_name_with_shell_metacharacters_stays_one_argument()
-> Result<(), Box<dyn std::error::Error>> {
    let hostile = "agent/x;$(touch /tmp/pwned) rm -rf .";
    let commands = healthy()
        .on(
            "gh pr view 7799",
            &format!(
                r#"{{"number":7799,"state":"MERGED","merged":true,"headRefName":"{hostile}","headRefOid":"{HEAD_SHA}"}}"#
            ),
        )
        .on("git ls-remote origin", &format!("{HEAD_SHA}\trefs/heads/{hostile}\n"));

    let outcome = evaluate(&collect_request(&commands, 7799, "origin")?);
    assert_eq!(outcome.admission, DeletionAdmission::SafeToDelete, "{}", outcome.detail);

    let deleter = RecordingDeleter::default();
    execute_admitted_deletion(&commands, &deleter, &outcome)?;
    let invocations = deleter.invocations.borrow();
    assert!(
        invocations[0].contains(&hostile.to_string()),
        "the branch must appear as one intact argument: {:?}",
        invocations[0],
    );
    Ok(())
}

/// Identity is re-verified immediately before deleting, not merely at
/// collection time: a remote that no longer resolves to the admitted
/// repository must stop the deletion.
#[test]
fn the_deletion_path_reverifies_remote_identity() -> Result<(), Box<dyn std::error::Error>> {
    let outcome = evaluate(&collect_request(&healthy(), 7799, "origin")?);
    assert_eq!(outcome.admission, DeletionAdmission::SafeToDelete);

    let moved_remote =
        healthy().on("git remote get-url origin", "https://github.com/SomeoneElse/other.git\n");
    let deleter = RecordingDeleter::default();
    let result = execute_admitted_deletion(&moved_remote, &deleter, &outcome);
    assert!(result.is_err(), "a repository mismatch must refuse the deletion");
    assert!(deleter.invocations.borrow().is_empty(), "nothing may be executed once identity fails",);
    Ok(())
}
