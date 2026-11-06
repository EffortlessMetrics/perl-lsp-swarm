//! Generation-bound accepted runtime-limit view and consumer denominator (#16840).
//!
//! This module defines the consumer-facing contract later family cutovers
//! (`#16841`–`#16847`) migrate onto. It does not change provider results,
//! parser admission, deadlines, memory policy, or the mutable [`super::LSP_LIMITS`]
//! singleton. `#7479` remains the hard-envelope owner and `#10857` remains the
//! accepted-configuration-generation owner; identities here bind those
//! prerequisites without implementing them.
//!
//! Construction is sealed: the only production-shaped entry is
//! [`assemble::AcceptedLimitsAssembly::assemble`]. The view never exposes
//! raw JSON, client `u64` values, or a mutable [`super::LspLimits`] handle.

#![allow(dead_code, unused_imports)]

mod assemble;
mod change;
mod denominator;
mod family;
mod identity;
mod lookup;
mod values;
mod view;

#[cfg(test)]
mod tests;

pub(crate) use assemble::{AcceptedLimitsAssembly, AssemblyError};
pub(crate) use change::{RuntimeLimitsChange, RuntimeLimitsFamily};
pub(crate) use denominator::{
    ClampDisposition, DenominatorRow, DomainOwner, EnvelopeOwner, LimitConsumerState, LimitScope,
    RUNTIME_LIMITS_DENOMINATOR, denominator_violations,
};
pub(crate) use family::{
    DegradationPolicyFamily, IndexIoDeadlineFamily, MemoryCacheFamily, ProviderDeadlineFamily,
    ResultCapsFamily, SourceAdmissionFamily,
};
pub(crate) use identity::{
    ACCEPTED_RUNTIME_LIMITS_SCHEMA_GENERATION, ConfigurationGenerationIdentity,
    HardEnvelopeIdentity, ScopeIdentity,
};
pub(crate) use lookup::LimitLookup;
pub(crate) use values::{ByteBudget, CountThreshold, Deadline, ResultCap};
pub(crate) use view::{AcceptedRuntimeLimitsView, RelationalValidation};
