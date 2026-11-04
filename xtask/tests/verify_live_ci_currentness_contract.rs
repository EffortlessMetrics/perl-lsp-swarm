//! Currentness and native-group reformation contract for `verify-live-ci`.
//!
//! Preserve the #13350 classifier fixtures as historical refusal controls.
//! Current queue-only policy forbids branch-mutating fresh triggers, even when
//! a provider could produce one. Claude and Codex must express the same H/G
//! identity, lost-ack recovery, and missing-proof boundaries.

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

fn collapsed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn neutralize(text: &str) -> String {
    collapsed(&text.replace("`$", "`"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    NoOp,
    Wait,
    ClassifyFailure,
    NonMutatingRefresh,
    FreshTrigger,
    NotProven,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResultClass {
    ExistingEvidence,
    PrInFlight,
    MergeBlocked,
    NotProven,
}

#[derive(Clone, Copy)]
struct Observation {
    name: &'static str,
    unrelated_or_behind_only: bool,
    evidence_current: bool,
    pending_current_subject: bool,
    failed_current_subject: bool,
    material_merge_tree_changed: bool,
    ordinary_rerun_evaluates_new_subject: bool,
    exact_subject_dispatch_admissible: bool,
    policy_requires_new_subject: bool,
    provider_can_bind_fresh_trigger: bool,
    repository_policy_allows_fresh_trigger: bool,
    expected: (Action, ResultClass),
}

const FIXTURES: &[Observation] = &[
    Observation {
        name: "unrelated main movement must not create an empty commit",
        unrelated_or_behind_only: true,
        evidence_current: true,
        pending_current_subject: false,
        failed_current_subject: false,
        material_merge_tree_changed: false,
        ordinary_rerun_evaluates_new_subject: false,
        exact_subject_dispatch_admissible: false,
        policy_requires_new_subject: false,
        provider_can_bind_fresh_trigger: true,
        repository_policy_allows_fresh_trigger: true,
        expected: (Action::NoOp, ResultClass::ExistingEvidence),
    },
    Observation {
        name: "behind-only conflict-free candidate must not refresh or trigger",
        unrelated_or_behind_only: true,
        evidence_current: true,
        pending_current_subject: false,
        failed_current_subject: false,
        material_merge_tree_changed: false,
        ordinary_rerun_evaluates_new_subject: false,
        exact_subject_dispatch_admissible: false,
        policy_requires_new_subject: false,
        provider_can_bind_fresh_trigger: true,
        repository_policy_allows_fresh_trigger: true,
        expected: (Action::NoOp, ResultClass::ExistingEvidence),
    },
    Observation {
        name: "pending current-subject check must wait once without polling or mutation",
        unrelated_or_behind_only: false,
        evidence_current: false,
        pending_current_subject: true,
        failed_current_subject: false,
        material_merge_tree_changed: false,
        ordinary_rerun_evaluates_new_subject: false,
        exact_subject_dispatch_admissible: false,
        policy_requires_new_subject: false,
        provider_can_bind_fresh_trigger: true,
        repository_policy_allows_fresh_trigger: true,
        expected: (Action::Wait, ResultClass::PrInFlight),
    },
    Observation {
        name: "failed current-subject check cannot use a fresh trigger to seek green",
        unrelated_or_behind_only: false,
        evidence_current: false,
        pending_current_subject: false,
        failed_current_subject: true,
        material_merge_tree_changed: false,
        ordinary_rerun_evaluates_new_subject: false,
        exact_subject_dispatch_admissible: false,
        policy_requires_new_subject: false,
        provider_can_bind_fresh_trigger: true,
        repository_policy_allows_fresh_trigger: true,
        expected: (Action::ClassifyFailure, ResultClass::MergeBlocked),
    },
    Observation {
        name: "new-subject rerun or dispatch must be preferred over candidate mutation",
        unrelated_or_behind_only: false,
        evidence_current: false,
        pending_current_subject: false,
        failed_current_subject: false,
        material_merge_tree_changed: true,
        ordinary_rerun_evaluates_new_subject: true,
        exact_subject_dispatch_admissible: false,
        policy_requires_new_subject: true,
        provider_can_bind_fresh_trigger: true,
        repository_policy_allows_fresh_trigger: true,
        expected: (Action::NonMutatingRefresh, ResultClass::PrInFlight),
    },
    Observation {
        name: "stale-snapshot rerun with bindable writer-owned trigger is admissible",
        unrelated_or_behind_only: false,
        evidence_current: false,
        pending_current_subject: false,
        failed_current_subject: false,
        material_merge_tree_changed: true,
        ordinary_rerun_evaluates_new_subject: false,
        exact_subject_dispatch_admissible: false,
        policy_requires_new_subject: true,
        provider_can_bind_fresh_trigger: true,
        repository_policy_allows_fresh_trigger: true,
        expected: (Action::FreshTrigger, ResultClass::PrInFlight),
    },
    Observation {
        name: "unproven trigger/new-subject relationship remains NOT_PROVEN",
        unrelated_or_behind_only: false,
        evidence_current: false,
        pending_current_subject: false,
        failed_current_subject: false,
        material_merge_tree_changed: true,
        ordinary_rerun_evaluates_new_subject: false,
        exact_subject_dispatch_admissible: false,
        policy_requires_new_subject: true,
        provider_can_bind_fresh_trigger: false,
        repository_policy_allows_fresh_trigger: true,
        expected: (Action::NotProven, ResultClass::NotProven),
    },
    Observation {
        name: "queue-only repository policy forbids fresh trigger despite bindable provider mechanic",
        unrelated_or_behind_only: false,
        evidence_current: false,
        pending_current_subject: false,
        failed_current_subject: false,
        material_merge_tree_changed: true,
        ordinary_rerun_evaluates_new_subject: false,
        exact_subject_dispatch_admissible: false,
        policy_requires_new_subject: true,
        provider_can_bind_fresh_trigger: true,
        repository_policy_allows_fresh_trigger: false,
        expected: (Action::NotProven, ResultClass::NotProven),
    },
    Observation {
        name: "exact-subject dispatch that evaluates the changed tree is non-mutating",
        unrelated_or_behind_only: false,
        evidence_current: false,
        pending_current_subject: false,
        failed_current_subject: false,
        material_merge_tree_changed: true,
        ordinary_rerun_evaluates_new_subject: false,
        exact_subject_dispatch_admissible: true,
        policy_requires_new_subject: true,
        provider_can_bind_fresh_trigger: true,
        repository_policy_allows_fresh_trigger: true,
        expected: (Action::NonMutatingRefresh, ResultClass::PrInFlight),
    },
    Observation {
        name: "unrelated movement must not hide a pending current-subject check",
        unrelated_or_behind_only: true,
        evidence_current: false,
        pending_current_subject: true,
        failed_current_subject: false,
        material_merge_tree_changed: false,
        ordinary_rerun_evaluates_new_subject: false,
        exact_subject_dispatch_admissible: false,
        policy_requires_new_subject: false,
        provider_can_bind_fresh_trigger: true,
        repository_policy_allows_fresh_trigger: true,
        expected: (Action::Wait, ResultClass::PrInFlight),
    },
    Observation {
        name: "unrelated movement must not hide a failed current-subject check",
        unrelated_or_behind_only: true,
        evidence_current: false,
        pending_current_subject: false,
        failed_current_subject: true,
        material_merge_tree_changed: false,
        ordinary_rerun_evaluates_new_subject: false,
        exact_subject_dispatch_admissible: false,
        policy_requires_new_subject: false,
        provider_can_bind_fresh_trigger: true,
        repository_policy_allows_fresh_trigger: true,
        expected: (Action::ClassifyFailure, ResultClass::MergeBlocked),
    },
];

fn classify(obs: Observation) -> (Action, ResultClass) {
    if obs.pending_current_subject && !obs.material_merge_tree_changed {
        return (Action::Wait, ResultClass::PrInFlight);
    }
    if obs.failed_current_subject && !obs.material_merge_tree_changed {
        return (Action::ClassifyFailure, ResultClass::MergeBlocked);
    }
    if obs.unrelated_or_behind_only && !obs.material_merge_tree_changed {
        return (Action::NoOp, ResultClass::ExistingEvidence);
    }
    if obs.evidence_current
        && !obs.material_merge_tree_changed
        && !obs.pending_current_subject
        && !obs.failed_current_subject
    {
        return (Action::NoOp, ResultClass::ExistingEvidence);
    }
    if obs.pending_current_subject && !obs.material_merge_tree_changed {
        return (Action::Wait, ResultClass::PrInFlight);
    }
    if obs.failed_current_subject && !obs.material_merge_tree_changed {
        return (Action::ClassifyFailure, ResultClass::MergeBlocked);
    }
    if obs.material_merge_tree_changed {
        if obs.ordinary_rerun_evaluates_new_subject || obs.exact_subject_dispatch_admissible {
            return (Action::NonMutatingRefresh, ResultClass::PrInFlight);
        }
        if obs.policy_requires_new_subject
            && obs.provider_can_bind_fresh_trigger
            && obs.repository_policy_allows_fresh_trigger
        {
            return (Action::FreshTrigger, ResultClass::PrInFlight);
        }
        return (Action::NotProven, ResultClass::NotProven);
    }
    (Action::NotProven, ResultClass::NotProven)
}

const SKILL_MARKERS: &[&str] = &[
    "Preserve unchanged semantic review and local proof",
    "A changed H requires current eligibility and new exact-head admission",
    "G and invalidates old G execution evidence",
    "Do not rebase or create an empty commit for behind-only movement",
    "retain `NOT_PROVEN` and report that specific missing capability",
    "If native custody already exists for that H, observe it rather than enqueue again",
    "Lost acknowledgment is observation/recovery, never a blind second mutation",
    "Neither an associated PR nor a live queue prefix proves complete membership",
    "Never substitute a pre-queue workflow dispatch or an empty push",
    "only for exact persisted native custody/run state with an accountable owner and wake event",
    "A supported H publisher is admission evidence; it cannot replace actual G proof",
    "do not issue a second direct merge",
];

const FORBIDDEN_SKILL_MARKERS: &[&str] = &[
    "always push an empty commit when main advances",
    "poll unchanged pending checks",
    "retry failed current-subject checks with an empty commit until green",
    "Authorize one verified-clean empty commit",
];

fn validate_skill(body: &str) -> Result<(), String> {
    let decision = collapsed(section(body, "## Currentness and native group reformation")?);
    let live = collapsed(section(body, "## Live evidence classification")?);
    let order = collapsed(section(body, "## Required-status production order")?);
    let combined = format!("{decision} {live} {order}");
    for marker in SKILL_MARKERS {
        if !combined.contains(marker) {
            return Err(format!("lost currentness marker {marker:?}"));
        }
    }
    let whole = collapsed(body);
    for forbidden in FORBIDDEN_SKILL_MARKERS {
        if whole.contains(forbidden) {
            return Err(format!("restored forbidden currentness instruction {forbidden:?}"));
        }
    }
    Ok(())
}

#[test]
fn fixtures_cover_the_issue_negative_controls() {
    assert!(FIXTURES.len() >= 8, "keep at least one fixture per #13350 negative-control family");
    for fixture in FIXTURES {
        assert_eq!(classify(*fixture), fixture.expected, "{}", fixture.name);
    }
}

#[test]
fn unrelated_or_pending_or_failed_current_subject_cannot_select_fresh_trigger() {
    for fixture in FIXTURES {
        if fixture.unrelated_or_behind_only
            || fixture.pending_current_subject
            || fixture.failed_current_subject
            || fixture.ordinary_rerun_evaluates_new_subject
            || fixture.exact_subject_dispatch_admissible
        {
            assert_ne!(
                classify(*fixture).0,
                Action::FreshTrigger,
                "{} must not select a fresh trigger",
                fixture.name
            );
        }
    }
}

#[test]
fn repository_policy_forbids_fresh_trigger_when_provider_could_bind() -> Result<(), DynError> {
    let fixture = FIXTURES
        .iter()
        .copied()
        .find(|fixture| fixture.name.contains("queue-only repository policy"))
        .ok_or("repository-policy fixture")?;
    assert_eq!(classify(fixture), (Action::NotProven, ResultClass::NotProven));
    assert!(fixture.provider_can_bind_fresh_trigger);
    assert!(!fixture.repository_policy_allows_fresh_trigger);
    Ok(())
}

#[test]
fn queue_policy_refuses_branch_triggers_and_requires_new_group_proof() -> Result<(), DynError> {
    for fixture in FIXTURES {
        let queue_policy =
            Observation { repository_policy_allows_fresh_trigger: false, ..*fixture };
        assert_ne!(classify(queue_policy).0, Action::FreshTrigger, "{}", fixture.name);
    }
    let root = root()?;
    for (provider, path) in [
        ("Codex", ".agents/skills/verify-live-ci/SKILL.md"),
        ("Claude", ".claude/skills/verify-live-ci/SKILL.md"),
    ] {
        let body = read(&root, path)?;
        let decision = collapsed(section(&body, "## Currentness and native group reformation")?);
        for needle in [
            "Preserve unchanged semantic review and local proof",
            "A changed H requires current eligibility and new exact-head admission",
            "G and invalidates old G execution evidence",
            "retain `NOT_PROVEN`",
        ] {
            assert!(decision.contains(needle), "{provider} decision section missing {needle:?}");
        }
    }
    Ok(())
}

#[test]
fn unproven_binding_cannot_be_reported_as_current() -> Result<(), DynError> {
    let fixture = FIXTURES
        .iter()
        .copied()
        .find(|fixture| fixture.name.contains("unproven"))
        .ok_or("provider-unavailable fixture")?;
    assert_eq!(classify(fixture), (Action::NotProven, ResultClass::NotProven));
    assert_ne!(classify(fixture).1, ResultClass::ExistingEvidence);
    assert_ne!(classify(fixture).1, ResultClass::PrInFlight);
    Ok(())
}

#[test]
fn providers_encode_the_same_currentness_contract() -> Result<(), DynError> {
    let root = root()?;
    let mut decisions = Vec::new();
    for (provider, path) in [
        ("Codex", ".agents/skills/verify-live-ci/SKILL.md"),
        ("Claude", ".claude/skills/verify-live-ci/SKILL.md"),
    ] {
        let body = read(&root, path)?;
        validate_skill(&body).map_err(|error| format!("{provider}: {error}"))?;
        decisions.push(neutralize(section(&body, "## Currentness and native group reformation")?));
    }
    assert_eq!(
        decisions[0], decisions[1],
        "Claude and Codex currentness decision sections diverged"
    );
    Ok(())
}

#[test]
fn ratchet_rejects_empty_commit_and_lost_subject_invalidation() -> Result<(), DynError> {
    let root = root()?;
    let source = read(&root, ".claude/skills/verify-live-ci/SKILL.md")?;
    validate_skill(&source).map_err(std::io::Error::other)?;

    let lost = source.replacen(
        "G and invalidates old G execution evidence",
        "G but old execution evidence may be reused without checking its subject",
        1,
    );
    assert_ne!(lost, source);
    assert!(validate_skill(&lost).is_err(), "dropping changed-G proof invalidation must fail");

    let unrelated_push = format!("{source}\nalways push an empty commit when main advances\n");
    assert!(validate_skill(&unrelated_push).is_err(), "unrelated-main empty commits must fail");

    let retry_green =
        format!("{source}\nretry failed current-subject checks with an empty commit until green\n");
    assert!(validate_skill(&retry_green).is_err(), "retry-to-green fresh triggers must fail");

    let polling = format!("{source}\npoll unchanged pending checks\n");
    assert!(validate_skill(&polling).is_err(), "pending-state polling must fail");
    Ok(())
}

#[test]
fn ratchet_rejects_provider_decision_divergence() -> Result<(), DynError> {
    let root = root()?;
    let claude = read(&root, ".claude/skills/verify-live-ci/SKILL.md")?;
    let mut codex = read(&root, ".agents/skills/verify-live-ci/SKILL.md")?;
    validate_skill(&claude).map_err(std::io::Error::other)?;
    validate_skill(&codex).map_err(std::io::Error::other)?;

    codex = codex.replacen(
        "G and invalidates old G execution evidence",
        "G but reuse old execution evidence without rebinding",
        1,
    );
    assert!(validate_skill(&codex).is_err());
    assert_ne!(
        neutralize(section(&claude, "## Currentness and native group reformation")?),
        neutralize(section(&codex, "## Currentness and native group reformation")?)
    );
    Ok(())
}
