//! Stable quality-gate facade.
//!
//! Candidate evaluation is intentionally clock-free. The underlying proof
//! engine now applies the lifecycle adaptation natively through a
//! [`LifecycleOverlay`] seam; this facade simply reads the caller-supplied
//! policy once, builds the overlay, and hands off to the engine's
//! overlay-aware entry point.
//!
//! Rendering, freshness comparison, output writes, and exit classification
//! all live inside the engine. This file must not duplicate any of them —
//! the consolidation invariant is enforced by
//! `quality_gate_ownership_contract` in `xtask/tests/`.

#[path = "quality_gate.rs"]
mod implementation;

pub use implementation::{QualityGateArgs, QualityGateMode};

use color_eyre::eyre::Result;
use std::fs;

pub fn run(args: QualityGateArgs) -> Result<()> {
    // When the policy file is unreadable, fall through to the engine's
    // natural fail-closed path: it already produces a `status: "missing"`
    // receipt and a `quality_exception_policy_not_current` next action.
    // Computing an overlay in that case would only mask the missing-file
    // diagnosis with an empty overlay and a sentinel stamp.
    let overlay = match fs::read_to_string(&args.exception_policy) {
        Ok(raw) => Some(implementation::compute_lifecycle_overlay(&raw)?),
        Err(_) => None,
    };
    implementation::run_with_overlay(args, overlay)
}
