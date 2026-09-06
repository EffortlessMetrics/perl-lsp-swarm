//! Errors for the release trust-invariant registry checker (#9392).

use std::error::Error;
use std::fmt::{Display, Formatter};

/// One or more registry contract violations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistryError(String);

impl RegistryError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub(crate) fn from_violations(violations: Vec<String>) -> Result<(), Self> {
        if violations.is_empty() {
            return Ok(());
        }
        let mut message = format!(
            "release trust-invariants check failed with {} violation(s):",
            violations.len()
        );
        for violation in &violations {
            message.push_str("\n  - ");
            message.push_str(violation);
        }
        Err(Self(message))
    }
}

impl Display for RegistryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for RegistryError {}
