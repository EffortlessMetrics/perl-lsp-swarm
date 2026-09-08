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
    DeletionAdmission, DeletionExecutor, ReadOnlyCommands, RecheckGate, RemoteIdentity,
    branch_deletion_command, collect_request, evaluate, execute_admitted_deletion,
    parse_remote_identity, recheck_gate, repository_from_remote_url, verify_remote_identity,
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
        r#"{{"number":7799,"state":"MERGED","merged":true,"headRefName":"{BRANCH}","headRefOid":"{HEAD_SHA}","isCrossRepository":false}}"#
    )
}

/// Every read succeeds and reports an unencumbered subject.
fn healthy() -> FakeCommands {
    FakeCommands::default()
        .on(
            "git remote get-url origin",
            "https://github.com/EffortlessMetrics/perl-lsp-swarm.git\n",
        )
        // The deletion travels over the PUSH endpoint, which git reports
        // separately; collection reads both and requires them to agree.
        .on(
            "git remote get-url --push --all origin",
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
    let collected = collect_request(&healthy(), 7799, "origin")?;
    let outcome = evaluate(&collected.request);
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
    let expected = parse_remote_identity("https://github.com/EffortlessMetrics/perl-lsp-swarm.git")
        .unwrap_or_else(unreachable_identity);
    assert!(
        verify_remote_identity(&commands, "origin", &expected).is_ok(),
        "the matching repository must verify",
    );

    let other = parse_remote_identity("https://github.com/SomeoneElse/perl-lsp-swarm.git")
        .unwrap_or_else(unreachable_identity);
    assert!(
        verify_remote_identity(&commands, "origin", &other).is_err(),
        "a different repository must be refused, not accepted",
    );

    // Host is part of identity: same owner/name on another server must NOT
    // verify. Comparing only owner/name would accept this.
    let impostor =
        parse_remote_identity("https://evil.example.com/EffortlessMetrics/perl-lsp-swarm.git")
            .unwrap_or_else(unreachable_identity);
    assert_eq!(
        impostor.repository.render(),
        expected.repository.render(),
        "the fixture is only meaningful if owner/name match",
    );
    assert_ne!(impostor, expected, "identity must distinguish the host");
    assert!(
        verify_remote_identity(&commands, "origin", &impostor).is_err(),
        "a same-named repository on another host must be refused",
    );

    let unreadable = FakeCommands::default().failing("git remote get-url", "no such remote");
    assert!(
        verify_remote_identity(&unreadable, "origin", &expected).is_err(),
        "an unreadable remote must be an error, never a pass",
    );
}

/// Only reached if a fixture URL fails to parse, which would make the test
/// meaningless rather than merely failing.
fn unreachable_identity() -> RemoteIdentity {
    RemoteIdentity {
        scheme: "fixture-url-failed-to-parse".to_string(),
        host: "fixture-url-failed-to-parse".to_string(),
        port: None,
        repository: xtask::branch_deletion_admission::RepositoryId::new("invalid", "invalid"),
    }
}

/// Host, port and `user@` handling in remote identity.
#[test]
fn remote_identity_keeps_the_host_and_normalises_it() {
    let cases = [
        ("https://github.com/O/R.git", "github.com", "O/R"),
        ("git@github.com:O/R.git", "github.com", "O/R"),
        ("ssh://git@GitHub.com:22/O/R.git", "github.com", "O/R"),
        ("https://evil.example.com/O/R", "evil.example.com", "O/R"),
    ];
    for (url, host, repository) in cases {
        let identity = parse_remote_identity(url).unwrap_or_else(unreachable_identity);
        assert_eq!(identity.host, host, "host for {url}");
        assert_eq!(identity.repository.render(), repository, "repository for {url}");
    }

    for url in ["", "not-a-url", "https://github.com/onlyowner", "https:///O/R"] {
        assert!(parse_remote_identity(url).is_none(), "{url:?} must not parse");
    }
}

/// The endpoint is scheme + host + port, not host alone.
///
/// A deletion leased against one endpoint must not be redeemable against
/// another that merely shares `owner/name` and a hostname. Each negative
/// control below differs from the reference in exactly one component.
#[test]
fn remote_identity_distinguishes_the_whole_endpoint() {
    let reference =
        parse_remote_identity("https://github.com/O/R.git").unwrap_or_else(unreachable_identity);

    // An explicit default port is the same endpoint, not a second one —
    // otherwise the check would fire on cosmetic URL differences.
    let explicit_default =
        parse_remote_identity("https://github.com:443/O/R").unwrap_or_else(unreachable_identity);
    assert_eq!(reference, explicit_default, "an explicit default port is the same endpoint");

    // Negative control: alternate port.
    let alternate_port =
        parse_remote_identity("https://github.com:8443/O/R").unwrap_or_else(unreachable_identity);
    assert_eq!(alternate_port.host, reference.host, "the host alone cannot separate these");
    assert_eq!(
        alternate_port.repository.render(),
        reference.repository.render(),
        "owner/name alone cannot separate these",
    );
    assert_ne!(alternate_port, reference, "an alternate port must be a different endpoint");
    assert_ne!(alternate_port.render(), reference.render(), "the rendering must differ too");

    // Negative control: different scheme, same host, both port-less in text.
    // `git://` is unauthenticated and unencrypted; accepting it as HTTPS would
    // be a transport downgrade.
    let other_scheme =
        parse_remote_identity("git://github.com/O/R").unwrap_or_else(unreachable_identity);
    assert_eq!(other_scheme.host, reference.host, "the host alone cannot separate these");
    assert_ne!(other_scheme, reference, "a different scheme must be a different endpoint");

    // The scp-like form is SSH on 22, and is therefore not the HTTPS endpoint.
    let scp = parse_remote_identity("git@github.com:O/R.git").unwrap_or_else(unreachable_identity);
    assert_eq!(scp.scheme, "ssh", "the scp-like form speaks ssh");
    assert_eq!(scp.port, Some(22), "the scp-like form has no port of its own; ssh defaults to 22");
    assert_ne!(scp, reference, "ssh and https are different endpoints");
    // ...and it is the same endpoint as the explicit ssh URL it is shorthand for.
    let ssh_explicit = parse_remote_identity("ssh://git@GitHub.com:22/O/R.git")
        .unwrap_or_else(unreachable_identity);
    assert_eq!(scp, ssh_explicit, "scp-like shorthand and its explicit ssh URL are one endpoint");

    // A non-numeric tail after ':' is not a port and must not be discarded.
    assert!(
        parse_remote_identity("https://github.com:notaport/O/R")
            .is_some_and(|identity| identity.host.contains("notaport")),
        "a non-numeric port tail must stay part of the host, not vanish",
    );
}

/// An unreadable or unparseable child listing must retain. "The listing
/// failed" is not "there are no children".
#[test]
fn an_unreadable_child_listing_retains() -> Result<(), Box<dyn std::error::Error>> {
    let unreachable = healthy().failing("gh pr list", "gh: could not reach api.github.com");
    let outcome = evaluate(&collect_request(&unreachable, 7799, "origin")?.request);
    assert_eq!(outcome.admission, DeletionAdmission::RetainGraphNotProven, "{}", outcome.detail);
    assert_eq!(branch_deletion_command(&outcome), None);

    let garbled = healthy().on("gh pr list", "{not json");
    let outcome = evaluate(&collect_request(&garbled, 7799, "origin")?.request);
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
    let outcome = evaluate(&collect_request(&full_page, 7799, "origin")?.request);
    assert_eq!(
        outcome.admission,
        DeletionAdmission::RetainGraphNotProven,
        "a full page must not be read as a complete graph: {}",
        outcome.detail,
    );
    Ok(())
}

/// A child row whose state this build cannot read must retain, not vanish.
///
/// Dropping the row would shrink the graph while it still reported `Complete`,
/// so a malformed or newer `gh` listing could reach `SAFE_TO_DELETE` with an
/// unseen child. The row deliberately targets an unrelated base: a row that
/// parsed would not retain on its own, so only the unreadable-state signal can
/// produce retention here. That makes the test fail if the drop is restored.
#[test]
fn an_unreadable_child_state_retains_rather_than_vanishing()
-> Result<(), Box<dyn std::error::Error>> {
    let unknown_state = healthy().on(
        "gh pr list",
        r#"[{"number":8123,"state":"QUEUED","isDraft":false,"headRefName":"h8123","baseRefName":"other","mergeable":"MERGEABLE"}]"#,
    );
    let outcome = evaluate(&collect_request(&unknown_state, 7799, "origin")?.request);
    assert_eq!(
        outcome.admission,
        DeletionAdmission::RetainGraphNotProven,
        "an unreadable child state must not be dropped: {}",
        outcome.detail,
    );
    assert!(
        outcome.detail.contains("8123") && outcome.detail.contains("QUEUED"),
        "the detail must name the row it could not read: {}",
        outcome.detail,
    );
    assert_eq!(branch_deletion_command(&outcome), None);

    // Positive control: the same listing with a readable state on the same
    // unrelated base admits, proving the retention above comes from the
    // unreadable state and not from the row merely being present.
    let readable = healthy().on(
        "gh pr list",
        r#"[{"number":8123,"state":"OPEN","isDraft":false,"headRefName":"h8123","baseRefName":"other","mergeable":"MERGEABLE"}]"#,
    );
    let outcome = evaluate(&collect_request(&readable, 7799, "origin")?.request);
    assert_eq!(
        outcome.admission,
        DeletionAdmission::SafeToDelete,
        "a readable child on an unrelated base must not retain: {}",
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
    let outcome = evaluate(&collect_request(&with_child, 7799, "origin")?.request);
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
    let outcome = evaluate(&collect_request(&unreadable, 7799, "origin")?.request);
    assert_eq!(outcome.admission, DeletionAdmission::RetainBranchMoved, "{}", outcome.detail);

    let advanced = healthy().on(
        "git ls-remote origin",
        &format!("2222222222222222222222222222222222222222\trefs/heads/{BRANCH}\n"),
    );
    let outcome = evaluate(&collect_request(&advanced, 7799, "origin")?.request);
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
    let outcome = evaluate(&collect_request(&owned, 7799, "origin")?.request);
    assert_eq!(outcome.admission, DeletionAdmission::RetainGraphNotProven, "{}", outcome.detail);
    assert!(
        outcome.detail.contains("wt-7762"),
        "the detail must name the worktree: {}",
        outcome.detail
    );

    let unreadable = healthy().failing("git worktree list", "not a git repository");
    let outcome = evaluate(&collect_request(&unreadable, 7799, "origin")?.request);
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
        let outcome = evaluate(&collect_request(&commands, 7799, "origin")?.request);
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

/// A command surface that repoints the remote once verification has read it.
///
/// The first `remote get-url` reads return the admitted endpoint, so collection
/// and the pre-deletion re-verification both pass. Every later read returns a
/// different endpoint — the shape of `git remote set-url --push` landing in the
/// gap between the last check and the mutation.
struct RepointsAfterVerification {
    inner: FakeCommands,
    reads: RefCell<usize>,
    allow_before_repoint: usize,
}

impl ReadOnlyCommands for RepointsAfterVerification {
    fn capture(&self, program: &str, args: &[&str]) -> color_eyre::eyre::Result<String> {
        if args.first() == Some(&"remote") {
            let mut reads = self.reads.borrow_mut();
            *reads += 1;
            if *reads > self.allow_before_repoint {
                return Ok("https://evil.example.com/Other/Repo.git\n".to_string());
            }
        }
        self.inner.capture(program, args)
    }
}

/// The executed argv must not follow a remote repointed after verification.
///
/// This is the race the review named: verifying a remote NAME and then pushing
/// to that name lets `git remote set-url --push` redirect the deletion after
/// every read has passed, because git resolves the name again at push time.
/// Binding the argv to the captured URL removes the second resolution, so the
/// mutation cannot follow the repoint no matter when it lands.
#[test]
fn the_executed_argv_cannot_follow_a_remote_repointed_after_verification()
-> Result<(), Box<dyn std::error::Error>> {
    const ADMITTED: &str = "https://github.com/EffortlessMetrics/perl-lsp-swarm.git";

    // Positive control: with no repoint, the deletion executes and targets the
    // admitted endpoint — so a refusal below cannot be the trivial outcome.
    let collected = collect_request(&healthy(), 7799, "origin")?;
    let outcome = evaluate(&collected.request);
    let control = RecordingDeleter::default();
    execute_admitted_deletion(&healthy(), &control, &outcome, &collected.remote_identity)?;
    let control_argv = control.invocations.borrow().first().cloned().unwrap_or_default();
    assert!(control_argv.contains(&ADMITTED.to_string()), "control argv: {control_argv:?}");

    // Now repoint after every verification read has already succeeded. The
    // executor still runs — verification saw a consistent remote — but the argv
    // was fixed at collection and cannot be redirected.
    let repointing = RepointsAfterVerification {
        inner: healthy(),
        reads: RefCell::new(0),
        // Collection reads fetch + push; re-verification reads them again.
        allow_before_repoint: 4,
    };
    let deleter = RecordingDeleter::default();
    let _ = execute_admitted_deletion(&repointing, &deleter, &outcome, &collected.remote_identity);

    for argv in deleter.invocations.borrow().iter() {
        assert!(
            argv.contains(&ADMITTED.to_string()),
            "the deletion must still target the admitted endpoint: {argv:?}",
        );
        assert!(
            !argv.iter().any(|argument| argument.contains("evil.example.com")),
            "the deletion followed a remote repointed after verification: {argv:?}",
        );
        assert!(
            !argv.iter().any(|argument| argument == "origin"),
            "a remote name in the argv would be re-resolved at push time: {argv:?}",
        );
    }
    Ok(())
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
        let outcome = evaluate(&collect_request(&commands, 7799, "origin")?.request);
        let deleter = RecordingDeleter::default();
        let bound = collect_request(&commands, 7799, "origin")?.remote_identity;
        let result = execute_admitted_deletion(&commands, &deleter, &outcome, &bound);
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
    let outcome = evaluate(&collect_request(&healthy(), 7799, "origin")?.request);
    let deleter = RecordingDeleter::default();
    let bound = collect_request(&healthy(), 7799, "origin")?.remote_identity;
    execute_admitted_deletion(&healthy(), &deleter, &outcome, &bound)?;

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

    let outcome = evaluate(&collect_request(&commands, 7799, "origin")?.request);
    assert_eq!(outcome.admission, DeletionAdmission::SafeToDelete, "{}", outcome.detail);

    let deleter = RecordingDeleter::default();
    let bound = collect_request(&commands, 7799, "origin")?.remote_identity;
    execute_admitted_deletion(&commands, &deleter, &outcome, &bound)?;
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
    let outcome = evaluate(&collect_request(&healthy(), 7799, "origin")?.request);
    assert_eq!(outcome.admission, DeletionAdmission::SafeToDelete);

    let moved_remote = healthy()
        .on("git remote get-url origin", "https://github.com/SomeoneElse/other.git\n")
        .on("git remote get-url --push --all origin", "https://github.com/SomeoneElse/other.git\n");
    let deleter = RecordingDeleter::default();
    let bound = collect_request(&healthy(), 7799, "origin")?.remote_identity;
    let result = execute_admitted_deletion(&moved_remote, &deleter, &outcome, &bound);
    assert!(result.is_err(), "a repository mismatch must refuse the deletion");
    assert!(deleter.invocations.borrow().is_empty(), "nothing may be executed once identity fails");
    Ok(())
}

/// A remote whose push URL differs from its fetch URL must not be admitted.
///
/// `git remote get-url <remote>` reads the FETCH url, but `git push` honors
/// `remote.<name>.pushurl`. Verified against real git 2.43.0: a remote can
/// report `github.com/EffortlessMetrics/perl-lsp-swarm` for fetch and an
/// entirely different endpoint for push. Binding only the fetch URL would let
/// collection, the child graph, the branch tip and the identity re-check all
/// verify against endpoint A while the leased deletion is delivered to
/// endpoint B — the one thing the whole admission exists to prevent.
#[test]
fn a_divergent_push_url_is_refused() -> Result<(), Box<dyn std::error::Error>> {
    // Positive control first: the fixture is only meaningful if the SAME pair
    // admits, so a refusal below cannot come from the fetch URL alone.
    assert_eq!(
        evaluate(&collect_request(&healthy(), 7799, "origin")?.request).admission,
        DeletionAdmission::SafeToDelete,
        "agreeing fetch and push endpoints must still admit",
    );

    let divergent = healthy()
        .on("git remote get-url --push --all origin", "https://evil.example.com/Other/Repo.git\n");
    let collected = collect_request(&divergent, 7799, "origin");
    let error = collected.err().ok_or("a divergent push URL must refuse collection")?;
    let rendered = error.to_string();
    assert!(
        rendered.contains("pushes to") && rendered.contains("evil.example.com"),
        "the refusal must name the endpoint the deletion would have reached: {rendered}",
    );

    // The re-check immediately before deleting must refuse it too, not only
    // collection: pushurl can be reconfigured inside the window.
    let bound = collect_request(&healthy(), 7799, "origin")?.remote_identity;
    assert!(
        verify_remote_identity(&divergent, "origin", &bound).is_err(),
        "re-verification must refuse a remote whose push endpoint diverged",
    );

    let outcome = evaluate(&collect_request(&healthy(), 7799, "origin")?.request);
    let deleter = RecordingDeleter::default();
    assert!(
        execute_admitted_deletion(&divergent, &deleter, &outcome, &bound).is_err(),
        "the deletion path must refuse a divergent push endpoint",
    );
    assert!(
        deleter.invocations.borrow().is_empty(),
        "nothing may be executed once the push endpoint fails to verify",
    );
    Ok(())
}

/// Multiple push URLs must be refused, not silently reduced to the first.
///
/// git permits several `remote.<name>.pushurl` entries and `git push <remote>`
/// delivers to EVERY one. Verified against real git 2.43.0 with two bare
/// destinations: a single push created the ref in both, while
/// `get-url --push` without `--all` named only the first. Reading one URL
/// would therefore admit endpoint A while the deletion also reached an
/// entirely unexamined endpoint B.
#[test]
fn a_fan_out_of_push_urls_is_refused() -> Result<(), Box<dyn std::error::Error>> {
    // Positive control: a single push URL matching fetch still admits, so a
    // refusal below cannot come from the extra read itself.
    assert_eq!(
        evaluate(&collect_request(&healthy(), 7799, "origin")?.request).admission,
        DeletionAdmission::SafeToDelete,
        "one agreeing push URL must still admit",
    );

    // Two destinations, the FIRST of which is the legitimately admitted one —
    // so a check that reads only the first would pass and delete on both.
    let fan_out = healthy().on(
        "git remote get-url --push --all origin",
        "https://github.com/EffortlessMetrics/perl-lsp-swarm.git\nhttps://evil.example.com/Other/Repo.git\n",
    );
    let error = collect_request(&fan_out, 7799, "origin")
        .err()
        .ok_or("a push-URL fan-out must refuse collection")?;
    let rendered = error.to_string();
    assert!(
        rendered.contains("2 push URLs") && rendered.contains("evil.example.com"),
        "the refusal must name the count and the unexamined endpoint: {rendered}",
    );

    // A remote with no readable push URL is unreadable, never permissive.
    let none = healthy().on("git remote get-url --push --all origin", "\n");
    assert!(
        collect_request(&none, 7799, "origin").is_err(),
        "a remote with no push URL must refuse, not default to the fetch URL",
    );

    // The re-check before deleting must refuse the fan-out too: pushurl can be
    // added inside the window between admission and deletion.
    let bound = collect_request(&healthy(), 7799, "origin")?.remote_identity;
    assert!(
        verify_remote_identity(&fan_out, "origin", &bound).is_err(),
        "re-verification must refuse a remote that gained a second push URL",
    );
    let outcome = evaluate(&collect_request(&healthy(), 7799, "origin")?.request);
    let deleter = RecordingDeleter::default();
    assert!(
        execute_admitted_deletion(&fan_out, &deleter, &outcome, &bound).is_err(),
        "the deletion path must refuse a push-URL fan-out",
    );
    assert!(
        deleter.invocations.borrow().is_empty(),
        "nothing may be executed once the push endpoints fail to verify",
    );
    Ok(())
}

/// The deletion must be executed against the verified URL, not the remote NAME.
///
/// A remote name is mutable config that git re-resolves at push time. Verifying
/// that `origin` points at the admitted endpoint and then running
/// `git push origin --delete` leaves a window in which
/// `git remote set-url --push origin <elsewhere>` redirects the deletion after
/// every check has already passed. Binding the argv to the URL removes the
/// second resolution entirely.
#[test]
fn the_deletion_targets_the_verified_url_not_the_remote_name()
-> Result<(), Box<dyn std::error::Error>> {
    let collected = collect_request(&healthy(), 7799, "origin")?;
    let outcome = evaluate(&collected.request);
    assert_eq!(outcome.admission, DeletionAdmission::SafeToDelete, "{}", outcome.detail);

    let argv =
        branch_deletion_command(&outcome).ok_or("an admitted outcome must yield a command")?;
    let target = argv.get(2).ok_or("the push target is the third argument")?;

    assert_eq!(
        target, "https://github.com/EffortlessMetrics/perl-lsp-swarm.git",
        "the push target must be the verified URL",
    );
    assert_ne!(target, "origin", "pushing to the remote name re-resolves mutable config");
    assert!(
        !argv.iter().any(|argument| argument == "origin"),
        "no argument may be the mutable remote name: {argv:?}",
    );

    // Fail closed: an outcome carrying no verified endpoint yields no command,
    // so a snapshot can never authorize a push at all.
    let mut unbound = outcome.clone();
    unbound.push_endpoint = None;
    assert_eq!(
        branch_deletion_command(&unbound),
        None,
        "an outcome with no bound push endpoint must yield no deletion command",
    );
    Ok(())
}

/// A fork parent must retain even though every other subject reads clean.
///
/// `gh` reports `isCrossRepository` for such a pull request; the collector
/// carries it, and the decision refuses. Without this the deletion would
/// target a same-named branch in the admitted repository that the merge never
/// touched.
#[test]
fn a_cross_repository_parent_retains() -> Result<(), Box<dyn std::error::Error>> {
    let fork = healthy().on(
        "gh pr view 7799",
        &format!(
            r#"{{"number":7799,"state":"MERGED","merged":true,"headRefName":"{BRANCH}","headRefOid":"{HEAD_SHA}","isCrossRepository":true}}"#
        ),
    );
    let collected = collect_request(&fork, 7799, "origin")?;
    assert!(
        !collected.request.parent.head_in_admitted_repository,
        "the collector must carry the fork flag",
    );

    let outcome = evaluate(&collected.request);
    assert_eq!(outcome.admission, DeletionAdmission::RetainBranchMoved, "{}", outcome.detail);
    assert_eq!(branch_deletion_command(&outcome), None);

    // And the control: the same shape with the flag false is admitted, so this
    // test cannot pass because the fixture is broken.
    let same_but_owned = evaluate(&collect_request(&healthy(), 7799, "origin")?.request);
    assert_eq!(same_but_owned.admission, DeletionAdmission::SafeToDelete);
    Ok(())
}

/// A command surface on which a child pull request is opened *after* the
/// admitting read and *before* the pre-deletion re-read.
///
/// `gh pr list` reports no children on its first call and a live child on
/// every call after it. This is the interleaving the review named: opening a
/// pull request does not move the branch tip, so neither the remote lease nor
/// a tip comparison can observe it. Only a second graph read can.
struct ChildOpensAfterFirstRead {
    inner: FakeCommands,
    listings: RefCell<usize>,
}

impl ChildOpensAfterFirstRead {
    fn new() -> Self {
        Self { inner: healthy(), listings: RefCell::new(0) }
    }
}

impl ReadOnlyCommands for ChildOpensAfterFirstRead {
    fn capture(&self, program: &str, args: &[&str]) -> color_eyre::eyre::Result<String> {
        if program == "gh" && args.first() == Some(&"pr") && args.get(1) == Some(&"list") {
            let mut listings = self.listings.borrow_mut();
            *listings += 1;
            if *listings > 1 {
                return Ok(format!(
                    r#"[{{"number":7810,"state":"OPEN","isDraft":false,"headRefName":"agent/child-7810","baseRefName":"{BRANCH}","mergeable":"MERGEABLE"}}]"#
                ));
            }
        }
        self.inner.capture(program, args)
    }
}

/// Drive the `cleanup` sequence — collect, evaluate, collect again, evaluate
/// again, gate, and only then delete — against one command surface, and report
/// whether the executor was reached.
///
/// This composes the same units the `Cleanup` arm composes. It does not prove
/// the arm is wired this way; `the_cleanup_path_gates_its_deletion_on_the_re_read`
/// in the sibling suite reads the arm's source and asserts that ordering. The
/// two together are what make the property behavioural *and* reachable.
fn run_cleanup_sequence(
    commands: &dyn ReadOnlyCommands,
) -> Result<(bool, RecordingDeleter), Box<dyn std::error::Error>> {
    let deleter = RecordingDeleter::default();

    let collected = collect_request(commands, 7799, "origin")?;
    let outcome = evaluate(&collected.request);
    if !outcome.admission.admits_deletion() {
        return Ok((false, deleter));
    }

    let recollected = collect_request(commands, 7799, "origin")?;
    let recheck = evaluate(&recollected.request);
    if let RecheckGate::Retain { .. } = recheck_gate(&outcome, &recheck) {
        return Ok((false, deleter));
    }

    execute_admitted_deletion(commands, &deleter, &recheck, &recollected.remote_identity)?;
    Ok((true, deleter))
}

/// The falsifier the review asked for: a child opened between the admitting
/// read and the deletion leaves the branch undeleted, and the executor is
/// never reached at all.
///
/// # What this does and does not close
///
/// The authorization-to-mutation window has two halves. This closes the first:
/// a child appearing between the two graph reads is observed by the second read
/// and retains. The second half — a child opened after the *final* read and
/// before the push lands — is not closed by anything here and cannot be, because
/// no lease observes a new dependency edge and GitHub exposes no lock against
/// one. That remainder needs an integration lock or a deferred-deletion policy
/// and is #3957 / #6188's, which is why this PR relates to #12885 as `Advances`.
#[test]
fn a_child_opened_after_the_admitting_read_is_never_deleted_out_from_under()
-> Result<(), Box<dyn std::error::Error>> {
    // Positive control first. Without it this test would pass for the trivial
    // reason that the sequence never deletes anything.
    let (control_deleted, control_deleter) = run_cleanup_sequence(&healthy())?;
    assert!(control_deleted, "an unencumbered subject must still be deleted");
    assert_eq!(
        control_deleter.invocations.borrow().len(),
        1,
        "the control must reach the executor exactly once",
    );

    let racing = ChildOpensAfterFirstRead::new();
    let (deleted, deleter) = run_cleanup_sequence(&racing)?;

    assert!(!deleted, "a child opened after the admitting read must retain the branch");
    assert!(
        deleter.invocations.borrow().is_empty(),
        "the deletion must not be attempted at all: {:?}",
        deleter.invocations.borrow(),
    );
    assert!(
        *racing.listings.borrow() >= 2,
        "the sequence must read the child graph twice, or this proves nothing; read {} time(s)",
        racing.listings.borrow(),
    );

    // The child the first read missed is exactly the one the second read must
    // catch: evaluating the later graph alone retains, and names the child.
    let later = evaluate(&collect_request(&racing, 7799, "origin")?.request);
    assert!(!later.admission.admits_deletion());
    assert!(
        later.retained_children.iter().any(|child| child.number == 7810),
        "the retained packet must name the child that appeared: {:?}",
        later.retained_children,
    );
    Ok(())
}
