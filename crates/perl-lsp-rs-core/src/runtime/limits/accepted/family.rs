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
        self.workspace_symbols != other.workspace_symbols
            || self.references != other.references
            || self.completion != other.completion
            || self.document_symbols != other.document_symbols
    }

    pub(crate) fn presentation_changed(self, other: Self) -> bool {
        self.code_lenses != other.code_lenses
            || self.diagnostics_per_file != other.diagnostics_per_file
            || self.inlay_hints != other.inlay_hints
    }
}

/// Source/parser/index admission family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceAdmissionFamily {
    pub(crate) max_file_size: LimitLookup<ByteBudget>,
    pub(crate) max_symbols_per_file: LimitLookup<CountThreshold>,
    pub(crate) parse_storm_threshold: LimitLookup<CountThreshold>,
}

/// Interactive provider deadlines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProviderDeadlineFamily {
    pub(crate) reference_search: LimitLookup<Deadline>,
    pub(crate) semantic_tokens: LimitLookup<Deadline>,
    pub(crate) code_lens_resolve: LimitLookup<Deadline>,
    pub(crate) completion: LimitLookup<Deadline>,
}

/// Indexing and filesystem deadlines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IndexIoDeadlineFamily {
    pub(crate) file_index: LimitLookup<Deadline>,
    pub(crate) regex_scan: LimitLookup<Deadline>,
    pub(crate) filesystem: LimitLookup<Deadline>,
}

/// Memory/cache family. Warning and critical thresholds are one relational group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MemoryCacheFamily {
    pub(crate) warning_threshold: LimitLookup<ByteBudget>,
    pub(crate) critical_threshold: LimitLookup<ByteBudget>,
    pub(crate) ast_cache: LimitLookup<ByteBudget>,
}

/// Degradation-policy family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DegradationPolicyFamily {
    pub(crate) return_partial_on_timeout: LimitLookup<bool>,
    pub(crate) include_open_docs_when_degraded: LimitLookup<bool>,
}
