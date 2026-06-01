//! Native critic rule contract.
//!
//! These types define the Rust-native policy diagnostic surface that future
//! rules should target. They intentionally live beside the existing
//! subprocess-backed Perl::Critic adapter and built-in fallback so callers can
//! migrate rule-by-rule without changing runtime behavior in one large step.

mod native_contract;
mod native_registry;
mod native_suppressions;
mod rules;
#[cfg(test)]
mod tests;

pub use native_contract::{
    CriticCategory, CriticContext, CriticFinding, CriticFix, CriticRelatedInformation, CriticRule,
    CriticTextEdit, FixSafety,
};
pub use native_registry::{NativeCriticProfile, NativeCriticRegistry};
pub use native_suppressions::{CriticSuppression, CriticSuppressionMap, CriticSuppressionScope};

pub use rules::*;
