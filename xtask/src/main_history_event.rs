//! Read-only classification of one exact `main` push event.
//!
//! A protected-branch push carries two independent propositions that must not be
//! collapsed into one another:
//!
//! ```text
//! platform event   forced | ordinary | created | deleted
//! local graph      ancestor | diverged | unrelated | not_proven_* | instrument_failure
//! ```
//!
//! The August 15 false re-root diagnosis came from reading an incomplete local
//! graph as proof of destructive movement. This module therefore consumes
//! [`crate::git_ancestry`] for every graph claim, keeps the platform observation
//! beside it rather than folded into it, and reports an explicit agreement axis.
//! A shallow, partial, or object-incomplete checkout can never produce a
//! `fast_forward` or `non_fast_forward` verdict.
//!
//! The module observes a push after the fact. It is not prevention, and it does
//! not read or assert live branch-protection state: that authority belongs to
//! the `release_live_controls` observer.

use crate::git_ancestry::{AncestryDisposition, AncestryReceipt, classify_ancestry};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Versioned receipt schema emitted by the push-event detector.
pub const MAIN_HISTORY_EVENT_SCHEMA_VERSION: &str = "main_history_event.v1";

/// What the platform reported the push to be.
///
/// This axis records GitHub's own delivery. It is never inferred from the local
/// commit graph, so a receipt keeps the platform claim even when the graph
/// cannot be verified.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventShape {
    /// An ordinary, non-force push to an existing ref.
    Ordinary,
    /// The push was delivered with GitHub's `forced` flag set.
    Forced,
    /// The push created the ref; there is no prior commit to compare against.
    Created,
    /// The push deleted the ref.
    Deleted,
    /// The delivered event fields cannot describe a single coherent push.
    Invalid,
}

impl EventShape {
    /// Stable machine spelling used by human and JSON projections.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Ordinary => "ordinary",
            Self::Forced => "forced",
            Self::Created => "created",
            Self::Deleted => "deleted",
            Self::Invalid => "invalid",
        }
    }
}

/// Whether the platform event and the verified local graph can both be true.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventGraphAgreement {
    /// The delivered event and the proven graph are compatible.
    Agrees,
    /// History moved non-fast-forward without the platform reporting a force push.
    Contradicts,
    /// The platform reported a force push, but the graph proves no history was lost.
    ///
    /// This pair is recorded rather than resolved. Whether GitHub derives
    /// `forced` from the client's push option or from the server-side ref
    /// relationship is not settled by its published schema, and the two readings
    /// disagree about whether this combination is routine or anomalous. What the
    /// graph does prove is that the before commit is still contained in the after
    /// commit, so no history was lost — the detector states that and declines to
    /// infer the rest, instead of flattening the pair into either `Agrees` or
    /// `Contradicts` on an unverified premise.
    ForceReportedWithoutHistoryLoss,
    /// The local graph was not proven, so no agreement can be derived.
    CannotVerify,
}

impl EventGraphAgreement {
    /// Stable machine spelling used by human and JSON projections.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Agrees => "agrees",
            Self::Contradicts => "contradicts",
            Self::ForceReportedWithoutHistoryLoss => "force_reported_without_history_loss",
            Self::CannotVerify => "cannot_verify",
        }
    }

    /// Exit contribution for evidence that does not agree.
    ///
    /// A force push reported against a ref whose protection forbids one is worth
    /// an operator's attention even though no history was lost. It gets its own
    /// code rather than sharing `3` with an unprovable graph: those are opposite
    /// situations — here the graph *is* proven — and a shared code would force
    /// the hosted check to describe one of them wrongly.
    const fn exit_code(self) -> u8 {
        match self {
            Self::Agrees | Self::CannotVerify => 0,
            Self::Contradicts => 2,
            Self::ForceReportedWithoutHistoryLoss => 5,
        }
    }
}

/// The strongest movement verdict proved for one delivered push event.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryEventVerdict {
    /// The before commit is a proven ancestor of the after commit.
    FastForward,
    /// The before commit is proven not to be contained in the after commit.
    NonFastForward,
    /// The push created the ref.
    CreatedRef,
    /// The push deleted the ref.
    DeletedRef,
    /// The local graph was incomplete, so movement remains undecided.
    NotProven,
    /// The delivered event fields were not a usable subject.
    InvalidEvent,
    /// Inspection failed before a domain result could be proved.
    InstrumentFailure,
}

impl HistoryEventVerdict {
    /// Stable machine spelling used by human and JSON projections.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::FastForward => "fast_forward",
            Self::NonFastForward => "non_fast_forward",
            Self::CreatedRef => "created_ref",
            Self::DeletedRef => "deleted_ref",
            Self::NotProven => "not_proven",
            Self::InvalidEvent => "invalid_event",
            Self::InstrumentFailure => "instrument_failure",
        }
    }

    /// Stable process exit code for shell and workflow consumers.
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::FastForward | Self::CreatedRef => 0,
            Self::NonFastForward | Self::DeletedRef => 2,
            Self::NotProven => 3,
            Self::InvalidEvent | Self::InstrumentFailure => 4,
        }
    }
}

/// One delivered push event, exactly as the platform reported it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PushEvent<'a> {
    /// Fully qualified ref the push targeted, such as `refs/heads/main`.
    pub reference: &'a str,
    /// Commit the ref pointed at before the push.
    pub before: &'a str,
    /// Commit the ref points at after the push.
    pub after: &'a str,
    /// GitHub's `forced` payload flag.
    pub forced: bool,
    /// GitHub's `created` payload flag.
    pub created: bool,
    /// GitHub's `deleted` payload flag.
    pub deleted: bool,
}

/// Exact event observations, the graph they were checked against, and the verdict.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MainHistoryEventReceipt {
    /// Receipt schema identity.
    pub schema_version: String,
    /// Caller-supplied repository path.
    pub repository: String,
    /// Fully qualified ref the push targeted.
    pub reference: String,
    /// Caller-supplied before revision.
    pub before_input: String,
    /// Caller-supplied after revision.
    pub after_input: String,
    /// Resolved before commit SHA, when the object was available.
    pub before_sha: Option<String>,
    /// Resolved after commit SHA, when the object was available.
    pub after_sha: Option<String>,
    /// Platform event classification.
    pub event_shape: EventShape,
    /// GitHub's `forced` payload flag, retained verbatim.
    pub event_forced: bool,
    /// GitHub's `created` payload flag, retained verbatim.
    pub event_created: bool,
    /// GitHub's `deleted` payload flag, retained verbatim.
    pub event_deleted: bool,
    /// Full ancestry evidence, when a graph comparison was attempted.
    pub graph: Option<AncestryReceipt>,
    /// Ancestry disposition, lifted for consumers that only read the summary.
    pub graph_disposition: Option<AncestryDisposition>,
    /// Whether the platform event and proven graph are compatible.
    pub agreement: EventGraphAgreement,
    /// Strongest proved movement verdict.
    pub verdict: HistoryEventVerdict,
    /// Bounded explanation of the verdict.
    pub reason: String,
    /// Evidence limitations that bound the claim.
    pub limitations: Vec<String>,
}

impl MainHistoryEventReceipt {
    /// Stable process exit code combining verdict and agreement.
    ///
    /// A proven history rewrite that the platform did not report as forced is
    /// blocking even when the movement verdict alone would not be.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        self.verdict.exit_code().max(self.agreement.exit_code())
    }

    /// Whether this receipt must fail its hosted check.
    #[must_use]
    pub fn is_blocking(&self) -> bool {
        self.exit_code() != 0
    }

    /// Stable human projection of the same receipt used for JSON output.
    #[must_use]
    pub fn render_human(&self) -> String {
        let mut lines = vec![
            format!("main-history-event: {}", self.verdict.as_str()),
            format!("repository: {}", self.repository),
            format!("ref: {}", self.reference),
            format!(
                "before: {} -> {}",
                self.before_input,
                display_option(self.before_sha.as_deref())
            ),
            format!("after: {} -> {}", self.after_input, display_option(self.after_sha.as_deref())),
            format!("event: {}", self.event_shape.as_str()),
            format!(
                "event-flags: forced={} created={} deleted={}",
                self.event_forced, self.event_created, self.event_deleted
            ),
            format!(
                "graph: {}",
                self.graph_disposition.as_ref().map_or("not_compared", AncestryDisposition::as_str)
            ),
            format!("agreement: {}", self.agreement.as_str()),
            format!("reason: {}", self.reason),
        ];
        if let Some(graph) = self.graph.as_ref() {
            lines.push(format!("merge-base: {}", display_option(graph.merge_base.as_deref())));
            lines.push(format!("shallow: {}", display_bool_option(graph.is_shallow_repository)));
            lines.push(format!("partial-clone: {}", display_bool_option(graph.is_partial_clone)));
            lines.extend(graph.limitations.iter().map(|item| format!("graph-limitation: {item}")));
        }
        lines.extend(self.limitations.iter().map(|item| format!("limitation: {item}")));
        lines.push(String::new());
        lines.join("\n")
    }

    fn new(repository: &Path, event: &PushEvent<'_>) -> Self {
        Self {
            schema_version: MAIN_HISTORY_EVENT_SCHEMA_VERSION.to_string(),
            repository: normalize_path_text(&repository.to_string_lossy()),
            reference: event.reference.to_string(),
            before_input: event.before.to_string(),
            after_input: event.after.to_string(),
            before_sha: None,
            after_sha: None,
            event_shape: EventShape::Invalid,
            event_forced: event.forced,
            event_created: event.created,
            event_deleted: event.deleted,
            graph: None,
            graph_disposition: None,
            agreement: EventGraphAgreement::CannotVerify,
            verdict: HistoryEventVerdict::InvalidEvent,
            reason: "the delivered event was not classified".to_string(),
            limitations: Vec::new(),
        }
    }

    fn finish(
        mut self,
        shape: EventShape,
        verdict: HistoryEventVerdict,
        reason: impl Into<String>,
    ) -> Self {
        self.event_shape = shape;
        self.verdict = verdict;
        self.reason = reason.into();
        self
    }
}

/// Classify one delivered `main` push event against the local commit graph.
///
/// The classifier never fetches, deepens, or mutates repository state: every
/// graph claim comes from [`classify_ancestry`], which fails closed on a
/// shallow, partial, or object-incomplete checkout.
#[must_use]
pub fn classify_push_event(repository: &Path, event: &PushEvent<'_>) -> MainHistoryEventReceipt {
    let receipt = MainHistoryEventReceipt::new(repository, event);

    if let Some(problem) = invalid_event(event) {
        return receipt.finish(EventShape::Invalid, HistoryEventVerdict::InvalidEvent, problem);
    }

    let deleted = event.deleted || is_zero_object_id(event.after);
    let created = event.created || is_zero_object_id(event.before);

    if deleted {
        let mut receipt = receipt.finish(
            EventShape::Deleted,
            HistoryEventVerdict::DeletedRef,
            "the push deleted the protected ref",
        );
        receipt.limitations.push(
            "a deleted ref has no after commit, so no local graph comparison is possible"
                .to_string(),
        );
        return receipt;
    }

    if created {
        let mut receipt = receipt.finish(
            EventShape::Created,
            HistoryEventVerdict::CreatedRef,
            "the push created the ref, so there is no prior commit to compare against",
        );
        receipt.limitations.push(
            "ref creation cannot be compared to earlier history; a delete/recreate sequence is proven by its own preceding delete event, not by this one"
                .to_string(),
        );
        return receipt;
    }

    let graph = classify_ancestry(repository, event.before, event.after);
    let shape = if event.forced { EventShape::Forced } else { EventShape::Ordinary };
    let verdict = verdict_for(&graph.disposition);
    let agreement = agreement_for(event.forced, &graph.disposition);
    let reason = reason_for(verdict, agreement, &graph);

    let mut receipt = receipt;
    receipt.before_sha = graph.base_sha.clone();
    receipt.after_sha = graph.head_sha.clone();
    receipt.graph_disposition = Some(graph.disposition.clone());
    receipt.agreement = agreement;
    receipt.limitations.extend(limitations_for(event.forced, &graph.disposition));
    receipt.graph = Some(graph);
    receipt.finish(shape, verdict, reason)
}

fn verdict_for(disposition: &AncestryDisposition) -> HistoryEventVerdict {
    match disposition {
        AncestryDisposition::Ancestor => HistoryEventVerdict::FastForward,
        AncestryDisposition::Diverged | AncestryDisposition::Unrelated => {
            HistoryEventVerdict::NonFastForward
        }
        AncestryDisposition::NotProvenShallow
        | AncestryDisposition::NotProvenPartialClone
        | AncestryDisposition::NotProvenMissingObject => HistoryEventVerdict::NotProven,
        AncestryDisposition::InvalidInput => HistoryEventVerdict::InvalidEvent,
        AncestryDisposition::InstrumentFailure => HistoryEventVerdict::InstrumentFailure,
    }
}

/// Derive whether the platform flag and the proven graph can both be true.
///
/// The one combination impossible under any reading is history moving
/// non-fast-forward while the platform reported no force push: that is
/// `Contradicts`. Its mirror — a reported force push over a graph that proves a
/// fast-forward — is deliberately *not* folded into `Agrees`. GitHub's published
/// schema ("whether this push was a force push of the `ref`") does not settle
/// whether `forced` records the client's push option or the server-side ref
/// relationship, and the two readings disagree about whether that pair is routine
/// or anomalous. It gets its own state so the receipt reports what is proven —
/// no history was lost — without asserting the unproven part.
const fn agreement_for(forced: bool, disposition: &AncestryDisposition) -> EventGraphAgreement {
    match disposition {
        AncestryDisposition::Ancestor => {
            if forced {
                EventGraphAgreement::ForceReportedWithoutHistoryLoss
            } else {
                EventGraphAgreement::Agrees
            }
        }
        AncestryDisposition::Diverged | AncestryDisposition::Unrelated => {
            if forced {
                EventGraphAgreement::Agrees
            } else {
                EventGraphAgreement::Contradicts
            }
        }
        AncestryDisposition::NotProvenShallow
        | AncestryDisposition::NotProvenPartialClone
        | AncestryDisposition::NotProvenMissingObject
        | AncestryDisposition::InvalidInput
        | AncestryDisposition::InstrumentFailure => EventGraphAgreement::CannotVerify,
    }
}

fn limitations_for(forced: bool, disposition: &AncestryDisposition) -> Vec<String> {
    let mut limitations = Vec::new();
    if forced && matches!(disposition, AncestryDisposition::Ancestor) {
        limitations.push(
            "the platform reported a force push while the graph proves the before commit is still contained in the after commit: no history was lost, but the two observations are not reconciled here, because GitHub's schema does not settle whether `forced` records the push option or the server-side ref relationship"
                .to_string(),
        );
    }
    if matches!(
        disposition,
        AncestryDisposition::NotProvenShallow
            | AncestryDisposition::NotProvenPartialClone
            | AncestryDisposition::NotProvenMissingObject
    ) {
        limitations.push(
            "the local graph could not be proven, so the delivered event flags remain the only retained evidence about this push"
                .to_string(),
        );
    }
    limitations
}

fn reason_for(
    verdict: HistoryEventVerdict,
    agreement: EventGraphAgreement,
    graph: &AncestryReceipt,
) -> String {
    if matches!(agreement, EventGraphAgreement::Contradicts) {
        return format!(
            "history moved non-fast-forward while the platform reported no force push: {}",
            graph.reason
        );
    }
    if matches!(agreement, EventGraphAgreement::ForceReportedWithoutHistoryLoss) {
        return
            "the platform reported a force push, and the graph proves no history was lost; the ref should not accept a force push at all, so the report is surfaced rather than resolved"
                .to_string();
    }
    match verdict {
        HistoryEventVerdict::FastForward => {
            "the before commit is a proven ancestor of the after commit".to_string()
        }
        HistoryEventVerdict::NonFastForward => {
            format!("the before commit is not contained in the after commit: {}", graph.reason)
        }
        _ => graph.reason.clone(),
    }
}

fn invalid_event(event: &PushEvent<'_>) -> Option<String> {
    if event.reference.trim().is_empty() {
        return Some("the delivered event has no target ref".to_string());
    }
    if event.created && event.deleted {
        return Some("the delivered event is both created and deleted".to_string());
    }
    if is_zero_object_id(event.before) && is_zero_object_id(event.after) {
        return Some("the delivered event has neither a before nor an after commit".to_string());
    }
    // A create/delete flag short-circuits the graph comparison entirely, so a
    // flag that disagrees with its own object name must never be taken at face
    // value: trusting `created: true` beside a real before commit would skip
    // ancestry and report success over history the detector never examined.
    // Only this direction is rejected — an all-zero object name still implies
    // creation or deletion on its own, because there is genuinely no commit on
    // that side to compare against.
    if event.created && !is_zero_object_id(event.before) {
        return Some(
            "the delivered event claims ref creation but names a real before commit".to_string(),
        );
    }
    if event.deleted && !is_zero_object_id(event.after) {
        return Some(
            "the delivered event claims ref deletion but names a real after commit".to_string(),
        );
    }
    if event.before.trim().is_empty() {
        return Some("the delivered event has no before revision".to_string());
    }
    if event.after.trim().is_empty() {
        return Some("the delivered event has no after revision".to_string());
    }
    None
}

/// Whether a delivered revision is the all-zero object name.
///
/// GitHub spells an absent side of a push as an all-zero object name.
///
/// The length is checked against the two real digest widths — SHA-1 and SHA-256
/// — rather than accepting any run of zeros. Without that floor a stray `"0"`
/// from upstream env plumbing would be read as "ref created", silently skipping
/// the graph comparison for a push that had a perfectly good before commit.
fn is_zero_object_id(value: &str) -> bool {
    let value = value.trim();
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte == b'0')
}

fn display_option(value: Option<&str>) -> &str {
    value.unwrap_or("not_available")
}

fn display_bool_option(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "not_available",
    }
}

fn normalize_path_text(value: &str) -> String {
    value.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Context, Result, bail};
    use std::fs;
    use std::process::Command;

    const ZERO: &str = "0000000000000000000000000000000000000000";

    #[test]
    fn ordinary_fast_forward_push_passes() -> Result<()> {
        let repository = initialized_repository()?;
        let before = git(repository.path(), &["rev-parse", "HEAD"])?;
        commit_file(repository.path(), "second.txt", "second\n", "second")?;
        let after = git(repository.path(), &["rev-parse", "HEAD"])?;

        let receipt = classify_push_event(repository.path(), &event(&before, &after, false));

        assert_eq!(receipt.verdict, HistoryEventVerdict::FastForward);
        assert_eq!(receipt.event_shape, EventShape::Ordinary);
        assert_eq!(receipt.agreement, EventGraphAgreement::Agrees);
        assert_eq!(receipt.graph_disposition, Some(AncestryDisposition::Ancestor));
        assert_eq!(receipt.before_sha.as_deref(), Some(before.as_str()));
        assert_eq!(receipt.after_sha.as_deref(), Some(after.as_str()));
        assert_eq!(receipt.exit_code(), 0);
        assert!(!receipt.is_blocking());
        Ok(())
    }

    /// A rewind to an earlier commit drops history even though both commits are
    /// related, so it must not be excused as an ordinary fast-forward.
    #[test]
    fn rewind_to_an_earlier_commit_is_non_fast_forward() -> Result<()> {
        let repository = initialized_repository()?;
        let first = git(repository.path(), &["rev-parse", "HEAD"])?;
        commit_file(repository.path(), "second.txt", "second\n", "second")?;
        let second = git(repository.path(), &["rev-parse", "HEAD"])?;

        let receipt = classify_push_event(repository.path(), &event(&second, &first, true));

        assert_eq!(receipt.verdict, HistoryEventVerdict::NonFastForward);
        assert_eq!(receipt.graph_disposition, Some(AncestryDisposition::Diverged));
        assert_eq!(receipt.exit_code(), 2);
        Ok(())
    }

    /// A re-rooted `main` is the exact incident shape this detector exists for.
    #[test]
    fn re_rooted_history_is_non_fast_forward() -> Result<()> {
        let repository = initialized_repository()?;
        let before = git(repository.path(), &["rev-parse", "HEAD"])?;
        git(repository.path(), &["switch", "--orphan", "rebuilt"])?;
        git(repository.path(), &["rm", "-rf", "--ignore-unmatch", "."])?;
        commit_file(repository.path(), "rebuilt.txt", "rebuilt\n", "rebuilt")?;
        let after = git(repository.path(), &["rev-parse", "HEAD"])?;

        let receipt = classify_push_event(repository.path(), &event(&before, &after, true));

        assert_eq!(receipt.verdict, HistoryEventVerdict::NonFastForward);
        assert_eq!(receipt.graph_disposition, Some(AncestryDisposition::Unrelated));
        assert_eq!(receipt.agreement, EventGraphAgreement::Agrees);
        assert!(receipt.is_blocking());
        Ok(())
    }

    /// History that moved non-fast-forward without a reported force push is the
    /// one genuinely impossible pair, and must block on the agreement axis.
    #[test]
    fn unforced_non_fast_forward_contradicts_the_delivered_event() -> Result<()> {
        let repository = initialized_repository()?;
        let first = git(repository.path(), &["rev-parse", "HEAD"])?;
        commit_file(repository.path(), "second.txt", "second\n", "second")?;
        let second = git(repository.path(), &["rev-parse", "HEAD"])?;

        let receipt = classify_push_event(repository.path(), &event(&second, &first, false));

        assert_eq!(receipt.agreement, EventGraphAgreement::Contradicts);
        assert_eq!(receipt.event_shape, EventShape::Ordinary);
        assert!(receipt.reason.contains("no force push"));
        assert_eq!(receipt.exit_code(), 2);
        Ok(())
    }

    /// A reported force push over a proven fast-forward is neither excused nor
    /// called a rewrite. Both axes stay readable, the pair gets its own state,
    /// and it surfaces for an operator instead of passing silently.
    #[test]
    fn force_reported_over_a_fast_forward_is_surfaced_not_resolved() -> Result<()> {
        let repository = initialized_repository()?;
        let before = git(repository.path(), &["rev-parse", "HEAD"])?;
        commit_file(repository.path(), "second.txt", "second\n", "second")?;
        let after = git(repository.path(), &["rev-parse", "HEAD"])?;

        let receipt = classify_push_event(repository.path(), &event(&before, &after, true));

        // The platform observation is retained verbatim ...
        assert_eq!(receipt.event_shape, EventShape::Forced);
        assert!(receipt.event_forced);
        // ... beside the independent graph result, neither overwriting the other.
        assert_eq!(receipt.graph_disposition, Some(AncestryDisposition::Ancestor));
        assert_eq!(receipt.verdict, HistoryEventVerdict::FastForward);
        // The pair is its own state: not flattened into agreement, not a rewrite.
        assert_eq!(receipt.agreement, EventGraphAgreement::ForceReportedWithoutHistoryLoss);
        assert_ne!(receipt.agreement, EventGraphAgreement::Agrees);
        assert_ne!(receipt.agreement, EventGraphAgreement::Contradicts);
        assert!(
            receipt.limitations.iter().any(|item| item.contains("no history was lost")),
            "the force-flag/fast-forward pair must be explained, not silently dropped"
        );
        assert!(receipt.is_blocking(), "a force push reported against main must not pass silently");
        assert_eq!(
            receipt.exit_code(),
            5,
            "its own code: the graph is proven here, unlike an unverifiable one at 3"
        );
        Ok(())
    }

    /// A create/delete flag skips the graph comparison entirely, so a flag that
    /// disagrees with its own object name must fail closed. Trusting
    /// `created: true` beside a real before commit would report success over a
    /// re-rooted history the detector never examined.
    #[test]
    fn a_create_or_delete_flag_contradicting_its_object_name_is_invalid() -> Result<()> {
        let repository = initialized_repository()?;
        let before = git(repository.path(), &["rev-parse", "HEAD"])?;
        git(repository.path(), &["switch", "--orphan", "rebuilt"])?;
        git(repository.path(), &["rm", "-rf", "--ignore-unmatch", "."])?;
        commit_file(repository.path(), "rebuilt.txt", "rebuilt\n", "rebuilt")?;
        let after = git(repository.path(), &["rev-parse", "HEAD"])?;

        // Without the flag this exact pair is a proven re-rooting.
        let honest = classify_push_event(repository.path(), &event(&before, &after, false));
        assert_eq!(honest.verdict, HistoryEventVerdict::NonFastForward);

        // A `created` flag must not turn it into a green created_ref.
        let mut lying = event(&before, &after, false);
        lying.created = true;
        let receipt = classify_push_event(repository.path(), &lying);
        assert_eq!(receipt.verdict, HistoryEventVerdict::InvalidEvent);
        assert_ne!(receipt.verdict, HistoryEventVerdict::CreatedRef);
        assert!(receipt.is_blocking(), "a contradicted creation flag must never report success");

        // The same for a `deleted` flag naming a live after commit.
        let mut deleted = event(&before, &after, false);
        deleted.deleted = true;
        let receipt = classify_push_event(repository.path(), &deleted);
        assert_eq!(receipt.verdict, HistoryEventVerdict::InvalidEvent);
        assert_ne!(receipt.verdict, HistoryEventVerdict::DeletedRef);
        Ok(())
    }

    /// A short run of zeros is not an object name. Reading `"0"` as one would
    /// silently skip the graph for a push that had a real before commit.
    #[test]
    fn a_truncated_zero_value_is_not_a_zero_object_name() -> Result<()> {
        let repository = initialized_repository()?;
        let after = git(repository.path(), &["rev-parse", "HEAD"])?;

        let receipt = classify_push_event(repository.path(), &event("0", &after, false));

        assert_ne!(
            receipt.verdict,
            HistoryEventVerdict::CreatedRef,
            "a one-character `0` must not be mistaken for a created ref"
        );
        assert_eq!(receipt.verdict, HistoryEventVerdict::NotProven);
        assert_eq!(receipt.graph_disposition, Some(AncestryDisposition::NotProvenMissingObject));
        assert!(receipt.is_blocking());
        Ok(())
    }

    /// The August 15 false incident: a shallow checkout can make an interior
    /// commit look root-like. That must never reach a movement verdict.
    #[test]
    fn shallow_checkout_is_not_proven_rather_than_non_fast_forward() -> Result<()> {
        let source = initialized_repository()?;
        commit_file(source.path(), "second.txt", "second\n", "second")?;
        commit_file(source.path(), "third.txt", "third\n", "third")?;
        let clone_parent = tempfile::tempdir()?;
        let clone = clone_parent.path().join("repository");
        let source_argument = source.path().to_string_lossy().into_owned();
        let clone_argument = clone.to_string_lossy().into_owned();
        git(
            clone_parent.path(),
            &["clone", "--depth", "1", "--no-local", &source_argument, &clone_argument],
        )?;

        let receipt = classify_push_event(&clone, &event("HEAD~2", "HEAD", false));

        assert_eq!(receipt.verdict, HistoryEventVerdict::NotProven);
        assert_eq!(receipt.graph_disposition, Some(AncestryDisposition::NotProvenShallow));
        assert_eq!(receipt.agreement, EventGraphAgreement::CannotVerify);
        assert_eq!(receipt.exit_code(), 3, "not_proven must be blocking, never green");
        Ok(())
    }

    /// A destructive push can remove the `before` object. The graph is then
    /// unknowable, but the delivered force flag must survive in the receipt.
    #[test]
    fn missing_before_object_keeps_the_delivered_force_flag() -> Result<()> {
        let repository = initialized_repository()?;
        let after = git(repository.path(), &["rev-parse", "HEAD"])?;

        let receipt = classify_push_event(
            repository.path(),
            &event("1111111111111111111111111111111111111111", &after, true),
        );

        assert_eq!(receipt.verdict, HistoryEventVerdict::NotProven);
        assert_eq!(receipt.graph_disposition, Some(AncestryDisposition::NotProvenMissingObject));
        assert_eq!(receipt.agreement, EventGraphAgreement::CannotVerify);
        assert_eq!(receipt.event_shape, EventShape::Forced);
        assert!(receipt.event_forced, "the platform force observation must not be erased");
        assert!(receipt.is_blocking());
        Ok(())
    }

    #[test]
    fn deleted_ref_is_an_explicit_failure_without_a_graph_claim() -> Result<()> {
        let repository = initialized_repository()?;
        let before = git(repository.path(), &["rev-parse", "HEAD"])?;
        let mut delivered = event(&before, ZERO, false);
        delivered.deleted = true;

        let receipt = classify_push_event(repository.path(), &delivered);

        assert_eq!(receipt.verdict, HistoryEventVerdict::DeletedRef);
        assert_eq!(receipt.event_shape, EventShape::Deleted);
        assert_eq!(receipt.graph, None, "a deleted ref has no after commit to compare");
        assert_eq!(receipt.exit_code(), 2);
        Ok(())
    }

    #[test]
    fn created_ref_is_distinct_from_an_ordinary_fast_forward() -> Result<()> {
        let repository = initialized_repository()?;
        let after = git(repository.path(), &["rev-parse", "HEAD"])?;
        let mut delivered = event(ZERO, &after, false);
        delivered.created = true;

        let receipt = classify_push_event(repository.path(), &delivered);

        assert_eq!(receipt.verdict, HistoryEventVerdict::CreatedRef);
        assert_ne!(receipt.verdict, HistoryEventVerdict::FastForward);
        assert_eq!(receipt.event_shape, EventShape::Created);
        assert_eq!(receipt.graph, None);
        Ok(())
    }

    /// The zero object name alone identifies a created or deleted ref even when
    /// the caller forwards no boolean flag.
    #[test]
    fn zero_object_name_identifies_creation_and_deletion_without_flags() -> Result<()> {
        let repository = initialized_repository()?;
        let head = git(repository.path(), &["rev-parse", "HEAD"])?;

        let created = classify_push_event(repository.path(), &event(ZERO, &head, false));
        let deleted = classify_push_event(repository.path(), &event(&head, ZERO, false));

        assert_eq!(created.verdict, HistoryEventVerdict::CreatedRef);
        assert_eq!(deleted.verdict, HistoryEventVerdict::DeletedRef);
        Ok(())
    }

    #[test]
    fn incoherent_event_fields_are_invalid_rather_than_classified() -> Result<()> {
        let repository = initialized_repository()?;
        let head = git(repository.path(), &["rev-parse", "HEAD"])?;

        let both = {
            let mut delivered = event(&head, &head, false);
            delivered.created = true;
            delivered.deleted = true;
            classify_push_event(repository.path(), &delivered)
        };
        let neither = classify_push_event(repository.path(), &event(ZERO, ZERO, false));
        let unreferenced = {
            let mut delivered = event(&head, &head, false);
            delivered.reference = "";
            classify_push_event(repository.path(), &delivered)
        };

        for receipt in [&both, &neither, &unreferenced] {
            assert_eq!(receipt.verdict, HistoryEventVerdict::InvalidEvent);
            assert_eq!(receipt.event_shape, EventShape::Invalid);
            assert_eq!(receipt.exit_code(), 4);
            assert_eq!(receipt.graph, None);
        }
        Ok(())
    }

    #[test]
    fn classification_does_not_mutate_the_repository() -> Result<()> {
        let repository = initialized_repository()?;
        let before = git(repository.path(), &["rev-parse", "HEAD"])?;
        commit_file(repository.path(), "second.txt", "second\n", "second")?;
        let after = git(repository.path(), &["rev-parse", "HEAD"])?;
        let reflog_before = git(repository.path(), &["reflog", "--all"])?;
        let status_before = git(repository.path(), &["status", "--porcelain"])?;

        let receipt = classify_push_event(repository.path(), &event(&before, &after, false));

        assert_eq!(receipt.verdict, HistoryEventVerdict::FastForward);
        assert_eq!(git(repository.path(), &["reflog", "--all"])?, reflog_before);
        assert_eq!(git(repository.path(), &["status", "--porcelain"])?, status_before);
        assert_eq!(git(repository.path(), &["rev-parse", "HEAD"])?, after);
        Ok(())
    }

    #[test]
    fn human_projection_retains_both_axes_and_the_subject() -> Result<()> {
        let repository = initialized_repository()?;
        let before = git(repository.path(), &["rev-parse", "HEAD"])?;
        commit_file(repository.path(), "second.txt", "second\n", "second")?;
        let after = git(repository.path(), &["rev-parse", "HEAD"])?;

        let rendered =
            classify_push_event(repository.path(), &event(&before, &after, true)).render_human();

        assert!(rendered.contains("main-history-event: fast_forward"));
        assert!(rendered.contains("event: forced"));
        assert!(rendered.contains("graph: ancestor"));
        assert!(rendered.contains("ref: refs/heads/main"));
        assert!(rendered.contains(&after));
        Ok(())
    }

    #[test]
    fn receipt_round_trips_through_its_json_projection() -> Result<()> {
        let repository = initialized_repository()?;
        let before = git(repository.path(), &["rev-parse", "HEAD"])?;
        commit_file(repository.path(), "second.txt", "second\n", "second")?;
        let after = git(repository.path(), &["rev-parse", "HEAD"])?;
        let receipt = classify_push_event(repository.path(), &event(&before, &after, false));

        let json = serde_json::to_string(&receipt)?;
        let restored: MainHistoryEventReceipt = serde_json::from_str(&json)?;

        assert_eq!(restored, receipt);
        assert_eq!(restored.schema_version, MAIN_HISTORY_EVENT_SCHEMA_VERSION);
        assert!(json.contains("\"verdict\":\"fast_forward\""));
        assert!(json.contains("\"agreement\":\"agrees\""));
        Ok(())
    }

    fn event<'a>(before: &'a str, after: &'a str, forced: bool) -> PushEvent<'a> {
        PushEvent {
            reference: "refs/heads/main",
            before,
            after,
            forced,
            created: false,
            deleted: false,
        }
    }

    fn initialized_repository() -> Result<tempfile::TempDir> {
        let repository = tempfile::tempdir()?;
        git(repository.path(), &["init", "--initial-branch", "main"])?;
        git(repository.path(), &["config", "user.name", "test"])?;
        git(repository.path(), &["config", "user.email", "test@example.com"])?;
        commit_file(repository.path(), "tracked.txt", "base\n", "base")?;
        Ok(repository)
    }

    fn commit_file(repository: &Path, path: &str, contents: &str, message: &str) -> Result<()> {
        fs::write(repository.join(path), contents)?;
        git(repository, &["add", "--", path])?;
        git(repository, &["commit", "-m", message])?;
        Ok(())
    }

    fn git(repository: &Path, arguments: &[&str]) -> Result<String> {
        let output = Command::new("git").args(arguments).current_dir(repository).output()?;
        if !output.status.success() {
            bail!(
                "git {} failed with status {}\nstderr:\n{}",
                arguments.join(" "),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        String::from_utf8(output.stdout)
            .context("git command returned non-UTF-8 output")
            .map(|value| value.trim().to_string())
    }
}
