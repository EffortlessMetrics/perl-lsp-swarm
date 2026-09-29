//! Family-grouped lookups. Relational groups stay grouped, not independent leaves.

use super::lookup::LimitLookup;
use super::values::{ByteBudget, CountThreshold, Deadline, ResultCap};

/// Read-query plus presentation result-count family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResultCapsFamily {
    pub(crate) workspace_symbols: LimitLookup<ResultCap>,
    pub(crate) references: LimitLookup<ResultCap>,
    pub(crate) completion: LimitLookup<ResultCap>,
    pub(crate) document_symbols: LimitLookup<ResultCap>,
    pub(crate) code_lenses: LimitLookup<ResultCap>,
    pub(crate) diagnostics_per_file: LimitLookup<ResultCap>,
    pub(crate) inlay_hints: LimitLookup<ResultCap>,
}

impl ResultCapsFamily {
    pub(crate) fn read_query_changed(self, other: Self) -> bool {
        self.workspace_symbols.behavior_ne(other.workspace_symbols)
            || self.references.behavior_ne(other.references)
            || self.completion.behavior_ne(other.completion)
            || self.document_symbols.behavior_ne(other.document_symbols)
    }

    pub(crate) fn presentation_changed(self, other: Self) -> bool {
        self.code_lenses.behavior_ne(other.code_lenses)
            || self.diagnostics_per_file.behavior_ne(other.diagnostics_per_file)
            || self.inlay_hints.behavior_ne(other.inlay_hints)
    }
}

/// Source/parser/index admission family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceAdmissionFamily {
    pub(crate) max_file_size: LimitLookup<ByteBudget>,
    pub(crate) max_symbols_per_file: LimitLookup<CountThreshold>,
    pub(crate) parse_storm_threshold: LimitLookup<CountThreshold>,
}

impl SourceAdmissionFamily {
    pub(crate) fn behavior_changed(self, other: Self) -> bool {
        self.max_file_size.behavior_ne(other.max_file_size)
            || self.max_symbols_per_file.behavior_ne(other.max_symbols_per_file)
            || self.parse_storm_threshold.behavior_ne(other.parse_storm_threshold)
    }
}

/// Interactive provider deadlines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProviderDeadlineFamily {
    pub(crate) reference_search: LimitLookup<Deadline>,
    pub(crate) semantic_tokens: LimitLookup<Deadline>,
    pub(crate) code_lens_resolve: LimitLookup<Deadline>,
    pub(crate) completion: LimitLookup<Deadline>,
}

impl ProviderDeadlineFamily {
    pub(crate) fn behavior_changed(self, other: Self) -> bool {
        self.reference_search.behavior_ne(other.reference_search)
            || self.semantic_tokens.behavior_ne(other.semantic_tokens)
            || self.code_lens_resolve.behavior_ne(other.code_lens_resolve)
            || self.completion.behavior_ne(other.completion)
    }
}

/// Indexing and filesystem deadlines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IndexIoDeadlineFamily {
    pub(crate) file_index: LimitLookup<Deadline>,
    pub(crate) regex_scan: LimitLookup<Deadline>,
    pub(crate) filesystem: LimitLookup<Deadline>,
}

impl IndexIoDeadlineFamily {
    pub(crate) fn behavior_changed(self, other: Self) -> bool {
        self.file_index.behavior_ne(other.file_index)
            || self.regex_scan.behavior_ne(other.regex_scan)
            || self.filesystem.behavior_ne(other.filesystem)
    }
}

/// Memory/cache family. Warning and critical thresholds are one relational group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MemoryCacheFamily {
    pub(crate) warning_threshold: LimitLookup<ByteBudget>,
    pub(crate) critical_threshold: LimitLookup<ByteBudget>,
    pub(crate) ast_cache: LimitLookup<ByteBudget>,
}

impl MemoryCacheFamily {
    pub(crate) fn behavior_changed(self, other: Self) -> bool {
        self.warning_threshold.behavior_ne(other.warning_threshold)
            || self.critical_threshold.behavior_ne(other.critical_threshold)
            || self.ast_cache.behavior_ne(other.ast_cache)
    }
}

/// Degradation-policy family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DegradationPolicyFamily {
    pub(crate) return_partial_on_timeout: LimitLookup<bool>,
    pub(crate) include_open_docs_when_degraded: LimitLookup<bool>,
}

impl DegradationPolicyFamily {
    pub(crate) fn behavior_changed(self, other: Self) -> bool {
        self.return_partial_on_timeout.behavior_ne(other.return_partial_on_timeout)
            || self
                .include_open_docs_when_degraded
                .behavior_ne(other.include_open_docs_when_degraded)
    }
}
