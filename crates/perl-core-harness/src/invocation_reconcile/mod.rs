//! Per-invocation evidence reconciliation for the compiler harness (#8191).
//!
//! Joins terminal process evidence (#6884), canonical TAP facts
//! (`perl_test_facts`), and strict semantic runner records into one settled
//! invocation receipt whose domain outcome is recomputed rather than copied
//! from producer self-report.

mod derive;
mod model;
mod storage;

#[cfg(test)]
mod tests;

pub use derive::reconcile_invocation;
pub use model::{
    INVOCATION_RECEIPT_SCHEMA_VERSION, InvocationAuthority, InvocationBinding, InvocationDerived,
    InvocationDomainOutcome, InvocationProcessSummary, InvocationSemanticSummary,
    InvocationTapSummary, ReconcileInvocationInput, SemanticRecordRejection,
    SettledInvocationReceipt, TAP_PARSER_IDENTITY,
};
pub use storage::{invocation_receipt_path, publish_settled_invocation_receipt};
