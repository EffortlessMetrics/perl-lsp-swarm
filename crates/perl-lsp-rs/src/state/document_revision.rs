//! Staged accepted-source identity for #10247; production cutover belongs to #8286.
//!
//! Neither client versions, parser generations, content hashes nor synchronization
//! epochs may stand in for this identity. Allocation does not accept an open:
//! the future DocumentStore must call `open` only at its accepted-open boundary.

use std::cmp::Ordering;
use std::fmt;
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

/// An opaque open lifetime, unique only inside this process.
/// Deliberately distinct from the public semantic snapshot's hashed identity.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct OpenDocumentInstanceId(NonZeroU64);

impl fmt::Debug for OpenDocumentInstanceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OpenDocumentInstanceId(opaque)")
    }
}

/// Accepted source ordering within one instance; no bare ordering API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct DocumentGeneration(u64);

/// Immutable accepted-source identity. No serialization or implicit conversions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct DocumentRevision {
    instance: OpenDocumentInstanceId,
    generation: DocumentGeneration,
}

/// Bounded lifecycle refusal, without source, path, pointer or client payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RevisionError {
    InstanceExhausted,
    GenerationExhausted,
    DifferentInstance,
}

impl fmt::Display for RevisionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InstanceExhausted => "document instance allocator exhausted",
            Self::GenerationExhausted => "document generation exhausted",
            Self::DifferentInstance => "document revisions belong to different instances",
        })
    }
}

impl std::error::Error for RevisionError {}

// Last allocated ordinal. Zero means none allocated; MAX is permanently exhausted.
// There is exactly one production allocator and no production reset constructor.
static INSTANCES: InstanceAllocator = InstanceAllocator { last: AtomicU64::new(0) };

struct InstanceAllocator {
    last: AtomicU64,
}

impl InstanceAllocator {
    fn allocate(&self) -> Result<OpenDocumentInstanceId, RevisionError> {
        let previous = self
            .last
            .fetch_update(AtomicOrdering::Relaxed, AtomicOrdering::Relaxed, |last| {
                last.checked_add(1)
            })
            .map_err(|_| RevisionError::InstanceExhausted)?;
        let next = previous.checked_add(1).ok_or(RevisionError::InstanceExhausted)?;
        NonZeroU64::new(next).map(OpenDocumentInstanceId).ok_or(RevisionError::InstanceExhausted)
    }

    #[cfg(test)]
    fn after_for_test(last: u64) -> Self {
        Self { last: AtomicU64::new(last) }
    }
}

impl DocumentRevision {
    /// Mint one candidate accepted-open lifetime. The future store owns admission.
    pub(crate) fn open() -> Result<Self, RevisionError> {
        Ok(Self { instance: INSTANCES.allocate()?, generation: DocumentGeneration(0) })
    }

    /// Derive the next accepted-source candidate without modifying this revision.
    /// The future atomic commit owner decides whether the candidate is installed.
    pub(crate) fn checked_next(self) -> Result<Self, RevisionError> {
        let next = self.generation.0.checked_add(1).ok_or(RevisionError::GenerationExhausted)?;
        Ok(Self { generation: DocumentGeneration(next), ..self })
    }

    /// Order only revisions of the same lifetime. No cross-instance ordering exists.
    pub(crate) fn compare_same_instance(self, other: Self) -> Result<Ordering, RevisionError> {
        if self.instance != other.instance {
            return Err(RevisionError::DifferentInstance);
        }
        Ok(self.generation.0.cmp(&other.generation.0))
    }

    /// Diagnostic data only; never an admission token or durable cross-process ID.
    pub(crate) fn observation(self) -> RevisionObservation {
        RevisionObservation {
            scope: ObservationScope::CurrentProcessOnly,
            instance_ordinal: self.instance.0.get(),
            generation: self.generation.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObservationScope {
    /// Ordinals restart in another process. No cross-process equality is implied.
    CurrentProcessOnly,
}

/// Bounded in-memory projection, intentionally not Serialize/Deserialize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RevisionObservation {
    pub(crate) scope: ObservationScope,
    pub(crate) instance_ordinal: u64,
    pub(crate) generation: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_lsp_rs_core::protocol::document_version::ClientDocumentVersion;

    // Type-level negative controls: no numeric or client-domain substitution,
    // bare ordering, or durable serialization is available to consumers.
    static_assertions::assert_not_impl_any!(DocumentGeneration: Ord, PartialOrd, From<u64>, From<ClientDocumentVersion>);
    static_assertions::assert_not_impl_any!(DocumentRevision: Ord, PartialOrd, From<u64>, From<ClientDocumentVersion>, serde::Serialize);
    static_assertions::assert_not_impl_any!(OpenDocumentInstanceId: From<u64>, serde::Serialize);
    static_assertions::assert_not_impl_any!(RevisionObservation: serde::Serialize);
    #[cfg(feature = "incremental")]
    static_assertions::assert_not_impl_any!(DocumentGeneration: From<perl_parser::incremental::ParseGeneration>);
    #[cfg(feature = "incremental")]
    static_assertions::assert_not_impl_any!(DocumentRevision: From<perl_parser::incremental::ParseGeneration>);

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn require(value: bool, message: &str) -> TestResult {
        if value { Ok(()) } else { Err(message.into()) }
    }

    #[test]
    fn document_revision_allocator_issues_maximum_once_then_stays_exhausted() -> TestResult {
        let allocator = InstanceAllocator::after_for_test(u64::MAX - 1);
        let last = allocator.allocate()?;
        require(last.0.get() == u64::MAX, "maximum instance must be issued")?;
        for _ in 0..3 {
            require(
                allocator.allocate() == Err(RevisionError::InstanceExhausted),
                "exhaustion must never recycle an instance",
            )?;
        }
        Ok(())
    }

    #[test]
    fn document_revision_generation_reaches_maximum_without_wrap_or_repetition() -> TestResult {
        let initial = DocumentRevision::open()?;
        let before = DocumentRevision { generation: DocumentGeneration(u64::MAX - 1), ..initial };
        let last = before.checked_next()?;
        require(last.generation.0 == u64::MAX, "maximum generation must be reachable")?;
        require(before.compare_same_instance(last)? == Ordering::Less, "advance must order")?;
        require(
            last.checked_next() == Err(RevisionError::GenerationExhausted),
            "maximum must fail instead of repeating or wrapping",
        )?;
        require(before.generation.0 == u64::MAX - 1, "advance must not mutate predecessor")
    }

    #[test]
    fn document_revision_reopen_and_cross_instance_comparison_remain_distinct() -> TestResult {
        // Identical URI/client version/bytes cannot influence allocation: none
        // are allocator inputs. These two accepted-open candidates model reopen.
        let first = DocumentRevision::open()?;
        let reopened = DocumentRevision::open()?;
        require(first.generation == reopened.generation, "both begin at initial generation")?;
        require(first != reopened, "reopen must mint a different identity")?;
        require(
            first.compare_same_instance(reopened) == Err(RevisionError::DifferentInstance),
            "bare numeric equality cannot authorize currentness",
        )
    }

    #[test]
    fn document_revision_identical_content_does_not_collapse_transitions() -> TestResult {
        let first = DocumentRevision::open()?;
        let second = first.checked_next()?;
        let third = second.checked_next()?;
        require(first.generation.0 == 0, "initial generation is zero")?;
        require(second.generation.0 == 1 && third.generation.0 == 2, "sequential generations")?;
        require(first != second && second != third, "content-independent revision identity")?;
        require(second.compare_same_instance(first)? == Ordering::Greater, "reverse ordering")?;
        require(second.compare_same_instance(second)? == Ordering::Equal, "same revision equality")
    }

    #[test]
    fn document_revision_allocator_starts_nonzero_and_never_collides() -> TestResult {
        let allocator = InstanceAllocator::after_for_test(0);
        let first = allocator.allocate()?;
        let second = allocator.allocate()?;
        require(first.0.get() == 1 && second.0.get() == 2, "checked allocator begins at one")?;
        require(first != second, "sequential instances must differ")
    }

    #[test]
    fn document_revision_concurrent_allocation_reserves_unique_last_ids() -> TestResult {
        let allocator = InstanceAllocator::after_for_test(u64::MAX - 2);
        let mut ids = std::thread::scope(|scope| -> Result<Vec<_>, Box<dyn std::error::Error>> {
            let first = scope.spawn(|| allocator.allocate());
            let second = scope.spawn(|| allocator.allocate());
            Ok(vec![
                first.join().map_err(|_| "first allocator worker failed")??.0.get(),
                second.join().map_err(|_| "second allocator worker failed")??.0.get(),
            ])
        })?;
        ids.sort_unstable();
        require(ids == [u64::MAX - 1, u64::MAX], "concurrent reservations must not collide")?;
        require(
            allocator.allocate() == Err(RevisionError::InstanceExhausted),
            "concurrent exhaustion remains terminal",
        )
    }

    #[test]
    fn document_revision_observation_is_bounded_and_explicitly_local() -> TestResult {
        let revision = DocumentRevision::open()?.checked_next()?;
        let observation = revision.observation();
        require(observation.scope == ObservationScope::CurrentProcessOnly, "explicit scope")?;
        require(
            observation.instance_ordinal != 0 && observation.generation == 1,
            "exact observation",
        )?;
        require(
            format!("{:?}", revision.instance) == "OpenDocumentInstanceId(opaque)",
            "redacted debug",
        )?;
        require(format!("{observation:?}").len() < 160, "bounded observation")
    }
}
