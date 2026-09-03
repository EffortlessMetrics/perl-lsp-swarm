//! Errors for the critic rule-proof checker (#6973).

use std::error::Error;
use std::fmt::{Display, Formatter};

/// One or more rule-proof contract violations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProofError(String);

impl ProofError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub(crate) fn from_violations(violations: Vec<String>) -> Result<(), Self> {
        if violations.is_empty() {
            return Ok(());
        }
        let mut message =
            format!("critic rule-proof check failed with {} violation(s):", violations.len());
        for violation in &violations {
            message.push_str("\n  - ");
            message.push_str(violation);
        }
        Err(Self(message))
    }
}

impl Display for ProofError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ProofError {}
