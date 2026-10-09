//! Assemble an accepted view from a current `LspLimits` snapshot.

use super::super::LspLimits;
use super::family::{
    DegradationPolicyFamily, IndexIoDeadlineFamily, MemoryCacheFamily, ProviderDeadlineFamily,
    ResultCapsFamily, SourceAdmissionFamily,
};
use super::identity::{
    ConfigurationGenerationIdentity, HardEnvelopeIdentity, IdentityError, ScopeIdentity,
};
use super::lookup::LimitLookup;
use super::values::{ByteBudget, CountThreshold, Deadline, ResultCap};
use super::view::{AcceptedRuntimeLimitsView, RelationalValidation};

/// Inputs that bind #10857 / #7479 identities without implementing those owners.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AcceptedLimitsAssembly {
    configuration_generation: ConfigurationGenerationIdentity,
    hard_envelope: HardEnvelopeIdentity,
    scope: ScopeIdentity,
}

impl AcceptedLimitsAssembly {
    pub(crate) fn new(
        configuration_generation: ConfigurationGenerationIdentity,
        hard_envelope: HardEnvelopeIdentity,
        scope: ScopeIdentity,
    ) -> Self {
        Self { configuration_generation, hard_envelope, scope }
    }

    /// Bind identities as not-proven until #10857 / #7479 land.
    pub(crate) fn not_proven_prerequisites(scope: ScopeIdentity) -> Self {
        Self::new(
            ConfigurationGenerationIdentity::not_proven(
                "#10857 accepted configuration generation is not bound on current main",
            ),
            HardEnvelopeIdentity::not_proven(
                "#7479 product hard envelope is not bound on current main",
            ),
            scope,
        )
    }

    /// Map a current `LspLimits` snapshot into an immutable accepted view.
    ///
    /// This copies magnitudes; it does not retain the snapshot handle, apply
    /// envelope policy, or publish a second mutable store.
    pub(crate) fn assemble(
        &self,
        limits: &LspLimits,
    ) -> Result<AcceptedRuntimeLimitsView, AssemblyError> {
        let result_caps = ResultCapsFamily {
            workspace_symbols: LimitLookup::Accepted(ResultCap::from_accepted(
                limits.workspace_symbol_cap,
            )),
            references: LimitLookup::Accepted(ResultCap::from_accepted(limits.references_cap)),
            completion: LimitLookup::Accepted(ResultCap::from_accepted(limits.completion_cap)),
            document_symbols: LimitLookup::Accepted(ResultCap::from_accepted(
                limits.document_symbol_cap,
            )),
            code_lenses: LimitLookup::Accepted(ResultCap::from_accepted(limits.code_lens_cap)),
            diagnostics_per_file: LimitLookup::ParsedNoConsumer {
                snapshot: ResultCap::from_accepted(limits.diagnostics_per_file_cap),
            },
            inlay_hints: LimitLookup::Accepted(ResultCap::from_accepted(limits.inlay_hints_cap)),
        };
        let source_admission = SourceAdmissionFamily {
            max_file_size: LimitLookup::Accepted(ByteBudget::from_accepted(
                limits.max_file_size_bytes,
            )),
            max_symbols_per_file: LimitLookup::Transferred {
                snapshot: CountThreshold::from_accepted(limits.max_symbols_per_file),
                owner: "perl-workspace::IndexResourceLimits",
            },
            parse_storm_threshold: LimitLookup::Transferred {
                snapshot: CountThreshold::from_accepted(limits.parse_storm_threshold),
                owner: "perl-workspace::ParseStormMetrics",
            },
        };
        let provider_deadlines = ProviderDeadlineFamily {
            reference_search: LimitLookup::Accepted(Deadline::from_accepted(
                limits.reference_search_deadline,
            )),
            semantic_tokens: LimitLookup::Accepted(Deadline::from_accepted(
                limits.semantic_tokens_deadline,
            )),
            code_lens_resolve: LimitLookup::Accepted(Deadline::from_accepted(
                limits.code_lens_resolve_deadline,
            )),
            completion: LimitLookup::Accepted(Deadline::from_accepted(limits.completion_deadline)),
        };
        let index_io_deadlines = IndexIoDeadlineFamily {
            file_index: LimitLookup::FixedInternal {
                snapshot: Deadline::from_accepted(limits.file_index_deadline),
            },
            regex_scan: LimitLookup::ParsedNoConsumer {
                snapshot: Deadline::from_accepted(limits.regex_scan_deadline),
            },
            filesystem: LimitLookup::FixedInternal {
                snapshot: Deadline::from_accepted(limits.fs_operation_deadline),
            },
        };
        let memory_cache = MemoryCacheFamily {
            warning_threshold: LimitLookup::ParsedNoConsumer {
                snapshot: ByteBudget::from_accepted(limits.memory_budget.warning_threshold_bytes),
            },
            critical_threshold: LimitLookup::ParsedNoConsumer {
                snapshot: ByteBudget::from_accepted(limits.memory_budget.critical_threshold_bytes),
            },
            ast_cache: LimitLookup::ParsedNoConsumer {
                snapshot: ByteBudget::from_accepted(limits.memory_budget.ast_cache_max_bytes),
            },
        };
        let degradation_policy = DegradationPolicyFamily {
            return_partial_on_timeout: LimitLookup::FixedInternal {
                snapshot: limits.return_partial_on_timeout,
            },
            include_open_docs_when_degraded: LimitLookup::FixedInternal {
                snapshot: limits.include_open_docs_when_degraded,
            },
        };
        let relational = match (
            memory_cache.warning_threshold.snapshot(),
            memory_cache.critical_threshold.snapshot(),
        ) {
            (Some(warning), Some(critical)) if warning.get() < critical.get() => {
                RelationalValidation::Valid
            }
            (Some(_), Some(_)) => RelationalValidation::Invalid { group: "memory_thresholds" },
            _ => RelationalValidation::Invalid { group: "memory_thresholds" },
        };

        Ok(AcceptedRuntimeLimitsView::from_parts(super::view::AcceptedViewParts {
            configuration_generation: self.configuration_generation.clone(),
            hard_envelope: self.hard_envelope.clone(),
            scope: self.scope.clone(),
            result_caps,
            source_admission,
            provider_deadlines,
            index_io_deadlines,
            memory_cache,
            degradation_policy,
            relational,
            limitations: "requested-versus-accepted client values are not preserved on LspLimits; #7479/#10857 remain unbound",
        }))
    }
}

/// Assembly failures. Relational invalidity is recorded on the view, not raised here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AssemblyError {
    Identity(IdentityError),
}

impl From<IdentityError> for AssemblyError {
    fn from(error: IdentityError) -> Self {
        Self::Identity(error)
    }
}
