//! Deterministic family change classifier.

use super::view::AcceptedRuntimeLimitsView;

/// One runtime-limit family whose accepted contents moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RuntimeLimitsFamily {
    ReadResultCaps,
    PresentationResultCaps,
    SourceAdmission,
    ProviderDeadlines,
    IndexIoDeadlines,
    MemoryCache,
    DegradationPolicy,
}

/// Deterministic change set: identical accepted input yields [`RuntimeLimitsChange::Unchanged`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimeLimitsChange {
    Unchanged,
    Family(RuntimeLimitsFamily),
    Composite(Vec<RuntimeLimitsFamily>),
}

impl RuntimeLimitsChange {
    pub(crate) fn from_families(mut families: Vec<RuntimeLimitsFamily>) -> Self {
        families.sort();
        families.dedup();
        match families.len() {
            0 => Self::Unchanged,
            1 => Self::Family(families[0]),
            _ => Self::Composite(families),
        }
    }
}

impl AcceptedRuntimeLimitsView {
    /// Classify the family delta against `other`. Scope/generation/envelope
    /// identity is not a family change; callers compare identity separately.
    pub(crate) fn classify_change(&self, other: &Self) -> RuntimeLimitsChange {
        let mut families = Vec::new();
        if self.result_caps().read_query_changed(other.result_caps()) {
            families.push(RuntimeLimitsFamily::ReadResultCaps);
        }
        if self.result_caps().presentation_changed(other.result_caps()) {
            families.push(RuntimeLimitsFamily::PresentationResultCaps);
        }
        if self.source_admission() != other.source_admission() {
            families.push(RuntimeLimitsFamily::SourceAdmission);
        }
        if self.provider_deadlines() != other.provider_deadlines() {
            families.push(RuntimeLimitsFamily::ProviderDeadlines);
        }
        if self.index_io_deadlines() != other.index_io_deadlines() {
            families.push(RuntimeLimitsFamily::IndexIoDeadlines);
        }
        if self.memory_cache() != other.memory_cache() {
            families.push(RuntimeLimitsFamily::MemoryCache);
        }
        if self.degradation_policy() != other.degradation_policy() {
            families.push(RuntimeLimitsFamily::DegradationPolicy);
        }
        RuntimeLimitsChange::from_families(families)
    }
}
