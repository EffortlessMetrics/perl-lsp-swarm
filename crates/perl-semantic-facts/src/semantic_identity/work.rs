//! Common work-subject fields that later receipts bind to.
//!
//! This module defines only the shared lower vocabulary for identifying the
//! subject of semantic work. Retained/rebased/recomputed/fallback semantics
//! are owned by the #12122 incremental-impact successor and are deliberately
//! absent here.

use super::{SemanticFactFamily, SemanticIdentityContractError, SemanticSubjectGeneration};

use serde::{Deserialize, Serialize};

/// Instrument/budget state accompanying a work subject.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticInstrumentBudgetState {
    /// Instruments healthy within budget.
    Nominal,
    /// A budget threshold was reached.
    BudgetExhausted,
    /// The measuring instrument failed.
    InstrumentFailure,
    /// Instrumentation unavailable for this subject.
    Unavailable,
}

impl SemanticInstrumentBudgetState {
    /// Stable discriminant tag.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Nominal => "nominal",
            Self::BudgetExhausted => "budget-exhausted",
            Self::InstrumentFailure => "instrument-failure",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Identity of the producer/strategy performing semantic work.
///
/// Identifies who produced a result so receipts can distinguish producers; it
/// never upgrades the completeness of what was produced.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SemanticProducerStrategyIdentity {
    producer_id: String,
    strategy_id: String,
}

impl SemanticProducerStrategyIdentity {
    /// Construct a producer/strategy identity.
    ///
    /// # Errors
    /// Returns [`SemanticIdentityContractError::EmptyIdentityField`] when
    /// either component is empty.
    pub fn new(
        producer_id: impl Into<String>,
        strategy_id: impl Into<String>,
    ) -> Result<Self, SemanticIdentityContractError> {
        let producer_id = producer_id.into();
        let strategy_id = strategy_id.into();
        if producer_id.trim().is_empty() {
            return Err(SemanticIdentityContractError::EmptyIdentityField(
                "SemanticProducerStrategyIdentity.producer_id",
            ));
        }
        if strategy_id.trim().is_empty() {
            return Err(SemanticIdentityContractError::EmptyIdentityField(
                "SemanticProducerStrategyIdentity.strategy_id",
            ));
        }
        Ok(Self { producer_id, strategy_id })
    }

    /// Producer identifier.
    #[must_use]
    pub fn producer_id(&self) -> &str {
        &self.producer_id
    }

    /// Strategy identifier.
    #[must_use]
    pub fn strategy_id(&self) -> &str {
        &self.strategy_id
    }
}

/// Common work-subject identity bound by later semantic work receipts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SemanticWorkSubjectIdentity {
    subject: SemanticSubjectGeneration,
    producer_strategy: SemanticProducerStrategyIdentity,
    requested_fact_families: Vec<SemanticFactFamily>,
    scope_count: u64,
    contribution_count: u64,
    instrument_budget: SemanticInstrumentBudgetState,
}

impl SemanticWorkSubjectIdentity {
    /// Construct a work-subject identity.
    ///
    /// # Errors
    /// Returns a contract error when a duplicate fact family is requested.
    pub fn new(
        subject: SemanticSubjectGeneration,
        producer_strategy: SemanticProducerStrategyIdentity,
        requested_fact_families: Vec<SemanticFactFamily>,
        scope_count: u64,
        contribution_count: u64,
        instrument_budget: SemanticInstrumentBudgetState,
    ) -> Result<Self, SemanticIdentityContractError> {
        let mut sorted_families = requested_fact_families;
        let original_len = sorted_families.len();
        sorted_families.sort_by_key(|family| family.tag());
        sorted_families.dedup_by_key(|family| family.tag());
        if sorted_families.len() != original_len {
            return Err(SemanticIdentityContractError::ContradictoryStatus(
                "requested fact families must be distinct",
            ));
        }
        Ok(Self {
            subject,
            producer_strategy,
            requested_fact_families: sorted_families,
            scope_count,
            contribution_count,
            instrument_budget,
        })
    }

    /// Exact subject generation of the work.
    #[must_use]
    pub fn subject(&self) -> &SemanticSubjectGeneration {
        &self.subject
    }

    /// Producer/strategy identity of the worker.
    #[must_use]
    pub fn producer_strategy(&self) -> &SemanticProducerStrategyIdentity {
        &self.producer_strategy
    }

    /// Requested fact families, in canonical order.
    #[must_use]
    pub fn requested_fact_families(&self) -> &[SemanticFactFamily] {
        &self.requested_fact_families
    }

    /// Declared scope count of the work subject.
    #[must_use]
    pub fn scope_count(&self) -> u64 {
        self.scope_count
    }

    /// Declared contribution count of the work subject.
    #[must_use]
    pub fn contribution_count(&self) -> u64 {
        self.contribution_count
    }

    /// Instrument/budget state of the work subject.
    #[must_use]
    pub fn instrument_budget(&self) -> SemanticInstrumentBudgetState {
        self.instrument_budget
    }
}
