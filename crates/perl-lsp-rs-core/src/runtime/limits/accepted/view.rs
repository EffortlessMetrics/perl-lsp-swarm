//! Immutable accepted runtime-limit view.

use std::fmt::Write as _;

use crate::hashing::sha256_hex;

use super::family::{
    DegradationPolicyFamily, IndexIoDeadlineFamily, MemoryCacheFamily, ProviderDeadlineFamily,
    ResultCapsFamily, SourceAdmissionFamily,
};
use super::identity::{
    ACCEPTED_RUNTIME_LIMITS_SCHEMA_GENERATION, ConfigurationGenerationIdentity,
    HardEnvelopeIdentity, ScopeIdentity, push_tagged,
};
use super::lookup::LimitLookup;
use super::values::{ByteBudget, CountThreshold, Deadline, ResultCap};

/// Result of checking relational groups (currently memory warning < critical).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RelationalValidation {
    Valid,
    Invalid { group: &'static str },
}

/// Versioned accepted runtime-limit view. Immutable after assembly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AcceptedRuntimeLimitsView {
    schema_generation: u32,
    configuration_generation: ConfigurationGenerationIdentity,
    hard_envelope: HardEnvelopeIdentity,
    scope: ScopeIdentity,
    result_caps: ResultCapsFamily,
    source_admission: SourceAdmissionFamily,
    provider_deadlines: ProviderDeadlineFamily,
    index_io_deadlines: IndexIoDeadlineFamily,
    memory_cache: MemoryCacheFamily,
    degradation_policy: DegradationPolicyFamily,
    relational: RelationalValidation,
    limitations: &'static str,
    fingerprint: String,
}

/// Inputs required to seal an accepted view.
pub(crate) struct AcceptedViewParts {
    pub(crate) configuration_generation: ConfigurationGenerationIdentity,
    pub(crate) hard_envelope: HardEnvelopeIdentity,
    pub(crate) scope: ScopeIdentity,
    pub(crate) result_caps: ResultCapsFamily,
    pub(crate) source_admission: SourceAdmissionFamily,
    pub(crate) provider_deadlines: ProviderDeadlineFamily,
    pub(crate) index_io_deadlines: IndexIoDeadlineFamily,
    pub(crate) memory_cache: MemoryCacheFamily,
    pub(crate) degradation_policy: DegradationPolicyFamily,
    pub(crate) relational: RelationalValidation,
    pub(crate) limitations: &'static str,
}

impl AcceptedRuntimeLimitsView {
    pub(crate) fn from_parts(parts: AcceptedViewParts) -> Self {
        let schema_generation = ACCEPTED_RUNTIME_LIMITS_SCHEMA_GENERATION;
        let fingerprint = fingerprint_from_parts(schema_generation, &parts);
        Self {
            schema_generation,
            configuration_generation: parts.configuration_generation,
            hard_envelope: parts.hard_envelope,
            scope: parts.scope,
            result_caps: parts.result_caps,
            source_admission: parts.source_admission,
            provider_deadlines: parts.provider_deadlines,
            index_io_deadlines: parts.index_io_deadlines,
            memory_cache: parts.memory_cache,
            degradation_policy: parts.degradation_policy,
            relational: parts.relational,
            limitations: parts.limitations,
            fingerprint,
        }
    }

    pub(crate) const fn schema_generation(&self) -> u32 {
        self.schema_generation
    }

    pub(crate) fn configuration_generation(&self) -> &ConfigurationGenerationIdentity {
        &self.configuration_generation
    }

    pub(crate) fn hard_envelope(&self) -> &HardEnvelopeIdentity {
        &self.hard_envelope
    }

    pub(crate) fn scope(&self) -> &ScopeIdentity {
        &self.scope
    }

    pub(crate) fn result_caps(&self) -> ResultCapsFamily {
        self.result_caps
    }

    pub(crate) fn source_admission(&self) -> SourceAdmissionFamily {
        self.source_admission
    }

    pub(crate) fn provider_deadlines(&self) -> ProviderDeadlineFamily {
        self.provider_deadlines
    }

    pub(crate) fn index_io_deadlines(&self) -> IndexIoDeadlineFamily {
        self.index_io_deadlines
    }

    pub(crate) fn memory_cache(&self) -> MemoryCacheFamily {
        self.memory_cache
    }

    pub(crate) fn degradation_policy(&self) -> DegradationPolicyFamily {
        self.degradation_policy
    }

    pub(crate) fn relational(&self) -> RelationalValidation {
        self.relational
    }

    pub(crate) const fn limitations(&self) -> &'static str {
        self.limitations
    }

    pub(crate) fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

fn fingerprint_from_parts(schema_generation: u32, parts: &AcceptedViewParts) -> String {
    let mut material = String::new();
    let _ = write!(material, "schema={schema_generation};");
    parts.configuration_generation.push_material(&mut material);
    parts.hard_envelope.push_material(&mut material);
    parts.scope.push_material(&mut material);
    push_cap(&mut material, "ws", parts.result_caps.workspace_symbols);
    push_cap(&mut material, "ref", parts.result_caps.references);
    push_cap(&mut material, "cmp", parts.result_caps.completion);
    push_cap(&mut material, "dsym", parts.result_caps.document_symbols);
    push_cap(&mut material, "lens", parts.result_caps.code_lenses);
    push_cap(&mut material, "diag", parts.result_caps.diagnostics_per_file);
    push_cap(&mut material, "inlay", parts.result_caps.inlay_hints);
    push_bytes(&mut material, "filesize", parts.source_admission.max_file_size);
    push_count(&mut material, "symfile", parts.source_admission.max_symbols_per_file);
    push_count(&mut material, "storm", parts.source_admission.parse_storm_threshold);
    push_deadline(&mut material, "refdl", parts.provider_deadlines.reference_search);
    push_deadline(&mut material, "sem", parts.provider_deadlines.semantic_tokens);
    push_deadline(&mut material, "lensdl", parts.provider_deadlines.code_lens_resolve);
    push_deadline(&mut material, "cmpdl", parts.provider_deadlines.completion);
    push_deadline(&mut material, "idx", parts.index_io_deadlines.file_index);
    push_deadline(&mut material, "re", parts.index_io_deadlines.regex_scan);
    push_deadline(&mut material, "fs", parts.index_io_deadlines.filesystem);
    push_bytes(&mut material, "memw", parts.memory_cache.warning_threshold);
    push_bytes(&mut material, "memc", parts.memory_cache.critical_threshold);
    push_bytes(&mut material, "ast", parts.memory_cache.ast_cache);
    push_bool(&mut material, "partial", parts.degradation_policy.return_partial_on_timeout);
    push_bool(
        &mut material,
        "opendegraded",
        parts.degradation_policy.include_open_docs_when_degraded,
    );
    match parts.relational {
        RelationalValidation::Valid => material.push_str("rel=valid;"),
        RelationalValidation::Invalid { group } => {
            push_tagged(&mut material, "rel-invalid", group.as_bytes());
        }
    }
    push_tagged(&mut material, "lim", parts.limitations.as_bytes());
    sha256_hex(material.as_bytes())
}

fn push_cap(material: &mut String, key: &str, lookup: LimitLookup<ResultCap>) {
    push_lookup(material, key, lookup, |value| value.get().to_string());
}

fn push_bytes(material: &mut String, key: &str, lookup: LimitLookup<ByteBudget>) {
    push_lookup(material, key, lookup, |value| value.get().to_string());
}

fn push_count(material: &mut String, key: &str, lookup: LimitLookup<CountThreshold>) {
    push_lookup(material, key, lookup, |value| value.get().to_string());
}

fn push_deadline(material: &mut String, key: &str, lookup: LimitLookup<Deadline>) {
    push_lookup(material, key, lookup, |value| value.as_nanos().to_string());
}

fn push_bool(material: &mut String, key: &str, lookup: LimitLookup<bool>) {
    push_lookup(material, key, lookup, |value| usize::from(value).to_string());
}

fn push_lookup<T: Copy>(
    material: &mut String,
    key: &str,
    lookup: LimitLookup<T>,
    render: impl Fn(T) -> String,
) {
    match lookup {
        LimitLookup::Accepted(value) => {
            push_tagged(material, &format!("{key}-a"), render(value).as_bytes());
        }
        LimitLookup::FixedProduct(value) => {
            push_tagged(material, &format!("{key}-f"), render(value).as_bytes());
        }
        LimitLookup::FixedInternal { snapshot } => {
            push_tagged(material, &format!("{key}-i"), render(snapshot).as_bytes());
        }
        LimitLookup::ParsedNoConsumer { snapshot } => {
            push_tagged(material, &format!("{key}-p"), render(snapshot).as_bytes());
        }
        LimitLookup::Transferred { snapshot, owner } => {
            push_tagged(material, &format!("{key}-t"), render(snapshot).as_bytes());
            push_tagged(material, &format!("{key}-to"), owner.as_bytes());
        }
        LimitLookup::NotProven { snapshot, reason } => {
            match snapshot {
                Some(value) => {
                    push_tagged(material, &format!("{key}-n"), render(value).as_bytes());
                }
                None => material.push_str(&format!("{key}-n=none;")),
            }
            push_tagged(material, &format!("{key}-nr"), reason.as_bytes());
        }
    }
}
